# RoadWaves

RoadWaves is a Rust companion for Euro Truck Simulator 2 that routes a location-aware Radio Garden station into ETS2's existing Internet Radio player.

## Status

The Windows integration and automatic stock-map retuning are implemented. The desktop Control Center is being refined on the `dev` branch. Linux native packages are built by CI and remain pending real Fedora/Ubuntu ETS2 acceptance testing.

RoadWaves uses community-observed Radio Garden endpoints. Radio Garden is the only discovery provider, is not affiliated with this project, and may restrict or change those endpoints.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

The desktop UI is native Rust/egui. The service remains local-only: telemetry is received over loopback and the audio relay binds to loopback. No telemetry or logs are uploaded.

## Windows

The Windows CI job produces an x86_64 installer containing the GUI, CLI, native SCS telemetry plugin, documentation, and license. After installation, launch RoadWaves, choose the detected ETS2 profile, click **Install / Repair Integration**, start the service, and select **RoadWaves AutoTune** in ETS2's Internet Radio list.

## Linux

The Linux CI job builds native x86_64 binaries, the SCS `.so` plugin, and Ubuntu `.deb` / Fedora `.rpm` deliverables. Native Linux and Proton game behavior still require manual verification on the target distribution.

## Safety and reversibility

Integration edits are backup-safe and preserve unrelated radio entries and plugins. Uninstall removes only RoadWaves-owned files and retains recovery backups. The project is licensed under the MIT license; see `LICENSE`.

## Contributing

Use the `dev` branch for iteration. Run the local checks above before opening a pull request. Include the OS, ETS2 mode (native Windows, native Linux, or Proton), service log path, and `doctor --json` output with installation reports, after removing personal paths or tokens.
