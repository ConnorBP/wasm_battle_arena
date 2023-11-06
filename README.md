# ![ghost](assets/ghost.png "Sleepy") GHOST BATTLE ![ghost](assets/ghost.png "Wheepy")

A one versus one ghost battling game to the (extra?)death.

Designed in Bevy game engine initially based on the following game design tutorial series: https://johanhelsing.studio/posts/extreme-bevy
Project for me to learn the basics of the bevy ecs system as well as: peer2peer deterministic synchronized game states utilizing ggrs rollback, wave function map generation, deterministic random generation, and other novel game design concepts.

## Building
### Setup
- first install cargo and rust using [rustup](https://rustup.rs/)
- then add wasm target: `rustup target add wasm32-unknown-unknown`
- install wasm server runner `cargo install wasm-server-runner`
- install cargo watch `cargo install cargo-watch`
- install matchbox server `cargo install matchbox_server`
- run matchbox server in another window for dev testing: `matchbox_server`

### Build and run

- build with `cargo build --release --target wasm32-unknown-unknown`
- run with `cargo run --target wasm32-unknown-unknown`
- auto compile and test for development with `cargo watch -cx "run --release --target wasm32-unknown-unknown"`

## Deploying

- Requires wasm-bindgen-cli `cargo install wasm-bindgen-cli`
- optional wasm-opt: `cargo install wasm-opt --locked`
- then run the commands from deploy.bat


## Matchbox Server config

use the following service definition after installing matchbox_server for user matchbox on ubuntu 20+

`/etc/systemd/system/matchbox.your-domain.tld.service`

```
[Unit]
Description=matchbox_server service
[Service]
User=matchbox
Group=matchbox
WorkingDirectory=/home/matchbox/
Environment="HOST=0.0.0.0:3536"
ExecStart=/home/matchbox/.cargo/bin/matchbox_server
[Install]
WantedBy=multi-user.target
```
Then run: `sudo systemctl start matchbox.your-domain.tld.service`


```
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --out-dir ./out/ --target web ./target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm
```

### TODO

- [x] Add a block based map to the grid
- [x] Add collision detection to map using a simple calculation (coordinates / MAP_SIZE).floor() as index into array of blocktype at position
- [ ] fix literal corner case on collision detection which freezes movement on corners
- [ ] auto generate the map with wave collapse or perlin noise
- [x] update player spawn function with random locaion with no overlap
- [ ] check player spawn location generation with collision to not spawn in wall
- [ ] add touch screen / mobile controls and functionality
- [ ] add sound effects and subtle music