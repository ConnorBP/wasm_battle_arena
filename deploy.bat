ECHO BUILDING PROFILE RELEASE
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --out-dir ./out/ --target web ./target/wasm32-unknown-unknown/release/wasm_battle_arena.wasm
ECHO EXTRA OPTIMIZE
mkdir tmp
copy /b ".\\out\\wasm_battle_arena_bg.wasm" ".\\tmp\\wasm_battle_arena_tmp.wasm"
wasm-opt --strip-debug --dce --code-folding -Oz -ol 100 -s 100 -o ./out/wasm_battle_arena_bg.wasm ./tmp/wasm_battle_arena_tmp.wasm

rem ECHO CLEANUP
del ".\\tmp\\wasm_battle_arena_tmp.wasm"