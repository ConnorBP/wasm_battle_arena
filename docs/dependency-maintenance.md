# Dependency maintenance

Weekly Dependabot updates cover Cargo and GitHub Actions. Compatible security-related transitive updates should be accepted after native tests and the WebAssembly release build pass.

The remaining known advisory debt is coupled to major ecosystem work:

- `failure` is pulled through `ggrs 0.9 -> bitfield-rle`.
- `idna 0.4` is pulled through the Bevy 0.11-era `bevy_egui -> webbrowser -> url` stack.
- Bevy 0.11 and its plugins are coupled to the `reflect-states-0.11` fork patches, `bevy_roll_safe`, and the `egui-toast` fork.

Do not force incompatible transitive overrides. Upgrade Bevy, GGRS, bevy_ggrs, asset loading, Egui, audio, and the rollback forks together as a separate migration with deterministic gameplay and browser regression testing.
