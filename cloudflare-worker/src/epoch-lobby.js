import { DurableObject } from "cloudflare:workers";
import { createEpochState, startNextEpoch, submitReport, abort } from "./epoch-state.js";
import { MAX_LOBBY_SOCKETS, MAX_MESSAGE_BYTES, RECONNECT_GRACE_MS, applyMessageRateLimit, parseEpochClientMessage, parseEpochLobbyQuery, randomHex } from "./protocol.js";

const KEY="lobby-v3";
export class EpochLobby extends DurableObject {
  constructor(ctx,env){ super(ctx,env); this.ctx.blockConcurrencyWhile(async()=>{this.state=await ctx.storage.get(KEY)??null;}); }
  async fetch(request){
    if(request.method!=="GET"||request.headers.get("Upgrade")?.toLowerCase()!=="websocket") return new Response("WebSocket required",{status:426});
    const parsed=parseEpochLobbyQuery(new URL(request.url).searchParams); if(!parsed.ok)return new Response(parsed.error,{status:400});
    const now=Date.now(); await this.expire(now);
    if(!this.state)this.state=createEpochState(parsed.value.mode,parsed.value.capacity,now);
    if(this.state.mode!==parsed.value.mode||this.state.capacity!==parsed.value.capacity)return new Response("Lobby configuration mismatch",{status:409});
    let player,token;
    if(parsed.value.playerId){ player=this.state.players[parsed.value.playerId]; if(!player||player.tokenHash!==await hash(parsed.value.reconnectToken)||player.expired)return new Response("Invalid reconnect",{status:401}); token=randomHex(); player.tokenHash=await hash(token); player.connected=true; player.reconnectUntil=null; }
    else { if(this.live().length>=MAX_LOBBY_SOCKETS)return new Response("Lobby busy",{status:503}); const playerId=this.unique(); token=randomHex(); player={playerId,tokenHash:await hash(token),joinedAt:now,connected:true,ready:false,expired:false,profile:null,score:0,reconnectUntil:null}; this.state.players[playerId]=player; }
    const old=this.socket(player.playerId); if(old){const a=old.deserializeAttachment()??{};a.superseded=true;old.serializeAttachment(a);old.close(4001,"reconnected elsewhere");}
    const pair=new WebSocketPair();const [client,server]=Object.values(pair);this.ctx.acceptWebSocket(server);server.serializeAttachment({playerId:player.playerId,superseded:false,rate:{windowStarted:now,windowMessages:0}});
    await this.persist(); this.send(server,{type:"welcome",protocol:3,playerId:player.playerId,reconnectToken:token,reconnectGraceMs:RECONNECT_GRACE_MS}); this.status(server,player); return new Response(null,{status:101,webSocket:client});
  }
  async webSocketMessage(socket,raw){
    if(typeof raw!=="string"||new TextEncoder().encode(raw).byteLength>MAX_MESSAGE_BYTES)return this.violation(socket,"invalid message");
    const a=socket.deserializeAttachment();if(!a?.playerId||a.superseded)return this.violation(socket,"invalid session"); const limited=applyMessageRateLimit(a.rate,Date.now());a.rate=limited.rate;socket.serializeAttachment(a);if(!limited.allowed)return this.violation(socket,"rate exceeded");
    const parsed=parseEpochClientMessage(raw);if(!parsed.ok)return this.violation(socket,parsed.error);const m=parsed.value;const player=this.state.players[a.playerId];
    if(m.type==="ping")return this.send(socket,{type:"pong",nonce:m.nonce});
    if(m.type==="profile"){player.profile={name:m.name,paletteId:m.paletteId,cosmeticId:m.cosmeticId};await this.persist();this.send(socket,{type:"profile_accepted"});return;}
    if(m.type==="ready"){player.ready=true;let start=startNextEpoch(this.state,randomHex(),"roster_change");await this.persist();if(start)this.broadcastStart(start);else this.status(socket,player);return;}
    if(m.type==="signal"){const active=this.state.active;if(!active||m.epoch!==active.epoch||!active.roster.some(e=>e.playerId===a.playerId)||!active.roster.some(e=>e.playerId===m.to)||m.to===a.playerId)return this.error(socket,"invalid signal");const target=this.socket(m.to);if(!target)return this.error(socket,"target offline");this.send(target,{type:"signal",epoch:m.epoch,from:a.playerId,data:m.data});return;}
    if(m.type==="report"){const result=submitReport(this.state,a.playerId,m.epoch,m.round,m.outcomes,randomHex());await this.persist();if(result.type==="ack")this.send(socket,{type:"report_ack",...result});else if(result.type==="commit"){this.broadcast({type:"round_commit",...result});if(result.next)this.broadcastStart(result.next);}else if(result.type==="abort"){this.broadcast({type:"round_abort",...result});if(result.next)this.broadcastStart(result.next);}else this.error(socket,result.code);return;}
  }
  async webSocketClose(socket){await this.disconnect(socket);} async webSocketError(socket){await this.disconnect(socket);} async alarm(){await this.expire(Date.now());await this.persist();}
  async disconnect(socket){const a=socket.deserializeAttachment();if(!a?.playerId||a.superseded)return;if(this.socket(a.playerId,socket))return;const p=this.state.players[a.playerId];if(p){p.connected=false;p.reconnectUntil=Date.now()+RECONNECT_GRACE_MS;await this.persist();}}
  async expire(now){if(!this.state)return;for(const p of Object.values(this.state.players)){if(p.reconnectUntil&&p.reconnectUntil<=now){p.expired=true;p.ready=false;p.connected=false;p.reconnectUntil=null;if(this.state.active?.roster.some(e=>e.playerId===p.playerId)){const result=abort(this.state,"roster_member_expired",randomHex());this.broadcast({type:"round_abort",...result});if(result.next)this.broadcastStart(result.next);}}}}
  broadcastStart(active){this.broadcast({type:"start",protocol:3,epoch:active.epoch,round:active.round,mode:this.state.mode,capacity:this.state.capacity,seed:active.seed,roster:active.roster});}
  status(socket,p){this.send(socket,{type:"status",protocol:3,status:"waiting",mode:this.state.mode,capacity:this.state.capacity,active:this.state.active?{epoch:this.state.active.epoch,round:this.state.active.round}:null,ready:p.ready,score:p.score});}
  live(){return this.ctx.getWebSockets().filter(s=>s.readyState===1&&!s.deserializeAttachment()?.superseded);} socket(id,except=null){return this.live().find(s=>s!==except&&s.deserializeAttachment()?.playerId===id);} unique(){let id;do{id=randomHex();}while(this.state.players[id]);return id;}
  send(socket,msg){try{if(socket.readyState!==1)return false;socket.send(JSON.stringify(msg));return true;}catch{return false;}} broadcast(msg){for(const s of this.live())this.send(s,msg);} error(s,error){this.send(s,{type:"error",error});} violation(s,error){this.error(s,error);s.close(1008,error.slice(0,123));}
  async persist(){await this.ctx.storage.put(KEY,this.state);const next=Object.values(this.state.players).reduce((n,p)=>p.reconnectUntil?Math.min(n,p.reconnectUntil):n,Infinity);if(Number.isFinite(next))await this.ctx.storage.setAlarm(next);else await this.ctx.storage.deleteAlarm();}
}
async function hash(token){const d=await crypto.subtle.digest("SHA-256",new TextEncoder().encode(token));return Array.from(new Uint8Array(d),b=>b.toString(16).padStart(2,"0")).join("");}
