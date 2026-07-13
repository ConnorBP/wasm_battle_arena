# Asset provenance

- `assets/music/menu.ogg` is original Ghost Battle music composed by Warren Postma and approved by the project owner for use in the game. It currently plays continuously across menu and battle states.
- `ghost_base.png`, `ghost.png`, `eyes.png`, `bullet.png`, and `boom.png` are repository game art.
- `ghost_crown.png`, `ghost_wizard.png`, and `ghost_bow.png` are procedural 16×16 variations generated from `ghost_base.png` by `scripts/generate-cosmetics.py`. They preserve the source transparency and use nearest-neighbor pixel editing only.
- Existing OGG sound effects are repository game assets. New distinct music/SFX should include authorship or compatible-license details here before deployment.
- The Wave B arena floor, checker/noise/dither decoration, brick walls/mortar, shield bubble segments, and speed afterimage/sparks are generated entirely in repository Rust code (`src/game/assets/procedural.rs`, `src/game/map.rs`, `src/game/mod.rs`, and `src/game/player.rs`). They use only primitive sprites, the repository `ghost_base.png` afterimage silhouette, a restricted hard-coded palette, deterministic coordinate hashing, and nearest-neighbor rendering. No external art or runtime-fetched assets were added.
