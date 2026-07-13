import test from "node:test";
import assert from "node:assert/strict";
import { createEpochState, startNextEpoch, submitReport, selectNextRoster } from "../src/epoch-state.js";

function player(id, joinedAt) {
  return { playerId:id, joinedAt, connected:true, ready:true, expired:false, profile:{name:id,paletteId:0,cosmeticId:0}, score:0 };
}

test("mid-match join waits for next epoch", () => {
  const state=createEpochState("deathmatch",3,0);
  state.players.a=player("a",1); state.players.b=player("b",2); state.players.c=player("c",3);
  const first=startNextEpoch(state,"0".repeat(32),"initial");
  assert.deepEqual(first.roster.map(x=>x.playerId),["a","b","c"]);
  state.players.d=player("d",4);
  assert.deepEqual(state.active.roster.map(x=>x.playerId),["a","b","c"]);
  state.players.c.ready=false;
  const report=state.active.roster.map((entry,index)=>({playerId:entry.playerId,placement:index+1,scoreDelta:index===0?1:0}));
  assert.equal(submitReport(state,"a",0,0,report,"1".repeat(32)).type,"ack");
  assert.equal(submitReport(state,"b",0,0,[...report].reverse(),"1".repeat(32)).type,"ack");
  const commit=submitReport(state,"c",0,0,report,"1".repeat(32));
  assert.equal(commit.type,"commit");
  assert.deepEqual(commit.next.roster.map(x=>x.playerId),["a","b","d"]);
  assert.equal(commit.next.epoch,1);
});

test("conflicting reports abort without scores", () => {
  const state=createEpochState("duel",2,0); state.players.a=player("a",1); state.players.b=player("b",2);
  startNextEpoch(state,"0".repeat(32),"initial");
  const one=[{playerId:"a",placement:1,scoreDelta:1},{playerId:"b",placement:2,scoreDelta:0}];
  const two=[{playerId:"a",placement:2,scoreDelta:0},{playerId:"b",placement:1,scoreDelta:1}];
  assert.equal(submitReport(state,"a",0,0,one,"1".repeat(32)).type,"ack");
  assert.equal(submitReport(state,"b",0,0,two,"1".repeat(32)).type,"abort");
  assert.equal(state.players.a.score,0); assert.equal(state.players.b.score,0);
});

test("roster selection prefers incumbents then waiters", () => {
  const state=createEpochState("deathmatch",3,0); state.players.a=player("a",1); state.players.b=player("b",2); state.players.c=player("c",3); state.players.d=player("d",4);
  state.active={roster:[{playerId:"c"},{playerId:"a"},{playerId:"b"}]}; state.players.b.ready=false;
  assert.deepEqual(selectNextRoster(state),["a","c","d"]);
});
