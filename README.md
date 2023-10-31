# ![pac ghost](assets/ghost.png "Blinku") PAC BATTLE ![pac ghost](assets/ghost.png "Blinku")

A one versus one ghost battling game to the (extra?)death.

Designed in Bevy game engine based on the following game design tutorial series: https://johanhelsing.studio/posts/extreme-bevy

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
- then run the commands from deploy.bat

```
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --out-dir ./out/ --target web ./target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm
```

### TODO

- [ ] Add a block based map to the grid
- [ ] Add collision detection to map using a simple calculation (coordinates / MAP_SIZE).floor() as index into array of blocktype at position
- [ ] auto generate the map with wave collapse or perlin noise
- [ ] update player spawn function with random locaion
- [ ] check player spawn location generation with collision to not spawn in wall
- [ ] add touch screen / mobile controls and functionality
- [ ] add sound effects and subtle music