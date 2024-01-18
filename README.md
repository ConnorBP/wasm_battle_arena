# ![ghost](assets/textures/character/ghost.png "Sleepy") GHOST BATTLE ![ghost](assets/textures/character/ghost.png "Wheepy")

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

- [ ] apply more aggressive size reduction techniques to bevy
    - [x] wasm-opt on deploy
    - [x] LTO and opt-level in cargo
    - [x] prune some bevy features
    - [x] apply aggressive wasm-opt profile
        - [ ] add more optimize options
    - [ ] profile un used big functions and hit them with the `wasm-snip` tool
    - [x] tried wee_alloc and it ends up adding to file size funny enough
    - [ ] serve compressed with brotli or gzip

- [x] add a main menu and settings gui
    - [x] default matchmaking mode button 
    - [ ] manual ip connect mode
    - [ ] sync test option on dev build
    - [x] player settings such as:
        - [ ] name
        - [ ] color
        - [ ] cosmetics
        - [x] sfx and music volume control
- [ ] auto generate the map with wave collapse or perlin noise
- [ ] add more map tile types
    - [ ] create a pretty asset for the basic wall type and all of it's corners
    - [ ] make a pretty ground texture
    - [ ] some kind of out of bounds area texture to make it less boring. Or just make it black.
    - [ ] special block types such as: traps, or items pickups as tile types
- [ ] polish sound effects and music.
    - [ ] add more sfx
    - [ ] add more music (battle theme)
    - [ ] tweak death sound effect
- [x] fix literal corner case on collision detection which freezes movement on corners
    - [ ] it is possible to further improve this logic if i'm feeling bored
- [X] add touch screen / mobile controls and functionality
    - [ ] controls need some polishing. Make fire work while not moving.
- [x] polish and bug fix network issues and determinsm
    - `Key issues are fixed, for now, but stll keep an eye out for bugs!`
    - [ ] Improve Rollback Audio system robustness
- [ ] optimize performance

### Complete

- [x] add sound effects and subtle music
- [x] Add a block based map to the grid
- [x] Add collision detection to map using a simple calculation (coordinates / MAP_SIZE).floor() as index into array of blocktype at position
- [x] update player spawn function with random locaion with no overlap
- [X] check player spawn location generation with collision to not spawn in wall

### Additional Fun Features
- [ ] Cosmetics
- [ ] 
- [ ] 
- [ ] 
