# RoadWaves local implementation report

Implemented locally: Rust workspace, Radio Garden-only place client with `[longitude, latitude]` validation, explicit 55/25/10/10 geographic scoring and anti-switch threshold, safe idempotent `live_streams.sii` install/uninstall with backups, loopback MP3 endpoint, and CLI doctor/discovery/install/uninstall/serve commands.

CI workflows were intentionally omitted per request.

Rustup stable 1.99.0 is installed. The Git for Windows SDK is installed at `C:\git-sdk-64`, and the local GNU target uses the checked-in LLVM-MinGW wrapper with the SDK UCRT libraries.

Verified commands:

- `cargo fmt --all -- --check`: passed
- `cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`: passed
- `cargo +stable-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu -j 1`: passed, 47 tests (6 CLI, 40 core, 1 GUI)
- `cargo +stable-x86_64-pc-windows-gnu build --workspace --release --target x86_64-pc-windows-gnu -j 1`: passed
- CLI doctor: passed
- Loopback endpoint: HTTP 200, `audio/mpeg`, 12-byte MP3 fixture response
- SII smoke test: install, idempotent second install, uninstall, and preservation of an unrelated station: passed
- `roadwaves find-game`: passed; detected the current Windows Documents ETS2 profiles without modifying them
- CLI doctor game-binary discovery: passed; detected the current Windows ETS2 `bin/win_x64/eurotrucks2.exe` without modifying the installation
- CLI `doctor --json`: passed; structured profile/game-binary state matches the human-readable doctor output
- CLI synthetic telemetry simulator: passed; four versioned JSON packets from `tests/fixtures/routes/berlin-to-warsaw.json` were received by the loopback service
- Explicit plugin installer test: passed; scopes the DLL to `plugins/` and backs up an existing RoadWaves DLL
- Safe plugin uninstall/repair path: passed; removes only the RoadWaves plugin, retains a backup, and preserves unrelated plugins
- Plugin ABI target guard: passed; `.dll` sources are rejected for `bin/linux_x64`, `.so` sources are rejected for `bin/win_x64`, and uninstall selects the plugin format matching the selected target, preventing cross-platform payload mistakes.
- SII safety hardening: passed; missing/non-text files and malformed endpoints are rejected, and an edit made during preparation causes the write to abort instead of overwriting it.
- SII uninstall preservation: passed; removal is limited to the RoadWaves-owned entry, preserves CRLF/unrelated bytes, and aborts on concurrent file changes.
- Radio Garden live smoke request: reachable; returned 12,881 places. This verifies current endpoint reachability only, not in-game playback or station codec compatibility.
- Radio Garden client hardening: passed compilation/tests; JSON metadata is capped at 4 MiB, API requests identify JSON explicitly, and redirects are limited and rejected when their destination is unsafe.
- Metadata cache fallback: passed; `discover` uses a versioned per-user cache during provider outages, reports cached/stale state, and preserves corrupt cache files for diagnosis. Cached metadata never supplies live audio.
- Upstream relay safety smoke test: unsafe loopback URL rejected; validated upstream relay path compiles and retains the fixture fallback.
- Live Radio Garden channel smoke test: place `AHXkxjWD` normalized to one channel (`vEBRr_48`); stream probe returned an HTTPS URL with `audio/mpeg` and a valid MP3 signature.
- End-to-end live service smoke: passed; a normal no-fixture release service accepted a Berlin manual-location override, processed telemetry, selected live Radio Garden channel `0fY5rl7L` (`80s80s - Deutsch`), authenticated Hide/Unhide controls for that channel, and returned a real four-byte MP3 frame from `/radio.mp3`.
- AutoTune state-machine tests: passed; automatic cooldown, manual Next, Previous history, and Favorite Lock are covered.
- Location-aware AutoTune: passed; meaningful driving movement, quick travel, and new telemetry sessions clear the location cooldown so the next nearby station can be selected automatically.
- Manual three-anchor world-to-geography calibration tests: passed; interpolation works and collinear anchors are rejected.
- Built-in stock ETS2 map transform: passed; live telemetry coordinates resolve through the stock climate-data transform by default, while explicit three-anchor calibration remains available for custom maps.
- Calibration CLI/service smoke test: passed with a synthetic bounded calibration; accepted telemetry resolved to coordinates with `BoundedInterpolation` confidence, while unsupported coordinates remain unknown.
- Copied real installed ETS2 `live_streams.sii` smoke test: passed; RoadWaves entry inserted inside `live_stream_def`, while the live file remained unchanged.
- Combined `install-integration` smoke test: passed against a copied real profile; plugin destination, SII entry, and backup were all produced without touching the live game.
- Combined integration rollback: passed; a failed radio-file operation removes a newly copied plugin or restores the previous plugin bytes, preventing partial installs.
- Combined integration uninstall: passed; a failed radio-file removal leaves the plugin untouched, and a later plugin failure restores the radio file from its backup.
- Local Inno Setup build: passed; `dist\RoadWaves-0.1.0-windows-x64.exe` contains the executable, self-contained SCS DLL, documentation, and license.
- Windows installer privilege/runtime hardening: passed compile; default install location is per-user (`%LOCALAPPDATA%\Programs\RoadWaves`) and the statically linked plugin does not require a launcher-side MinGW runtime copy.
- Final Windows installer artifact: `dist\RoadWaves-0.1.0-windows-x64.exe`; its authoritative SHA-256 is recorded in the root `SHA256SUMS` manifest.
- Windows installer install/uninstall smoke: passed in a fresh disposable per-user directory; required binaries, self-contained plugin, README, license, and report were present, and the Inno uninstaller removed the test directory cleanly.
- Native GUI release build: passed; `roadwaves-gui.exe` is included in the installer with profile discovery, service status, AutoTune status, and honest verification messaging.
- GUI service control: passed compilation; Start/Stop launches and terminates the bundled local CLI service and reports spawn failures.
- GUI integration action: passed compilation; the GUI can install the backup-safe SCS plugin and `live_streams.sii` entry into the selected detected profile/game binary, with explicit status reporting.
- GUI installation workflow: passed compilation; detected profiles and game binaries are selectable, Install/Repair uses the selected pair, Uninstall removes only RoadWaves-owned files while retaining backups, and local settings/diagnostics are visible.
- GUI startup preference: passed compilation; the persisted opt-in preference starts the authenticated local service when the launcher opens.
- GUI mode controls: passed compilation/tests; AutoTune, Explore, Favorite Lock, Next, and Previous are wired to the typed AutoTune state machine, with no fake station success when no eligible list is loaded.
- GUI provider refresh: passed compilation/tests; startup and manual refresh use a background worker, display live/cached/stale Radio Garden metadata states, and keep the UI thread free of provider I/O.
- Authenticated GUI controls: passed raw loopback HTTP smoke; the GUI starts the service with a per-launch OS-random token, and AutoTune/Explore/Favorite Lock/Next/Previous commands are sent asynchronously to the service. Missing or incorrect tokens are rejected.
- Favorite control: implemented and linted; the authenticated GUI Favorite action toggles the active service candidate's favorite state and persists its channel snapshot in the per-user config.
- Hidden-channel control: implemented and linted; authenticated Hide/Unhide actions persist channel IDs, immediately advance away from a newly hidden station, and expose saved hidden channels in the GUI.
- Favorite/settings reload: passed unit coverage; the service restores saved favorites, hidden channels, and mode before discovering candidates, and service-side Favorite toggles update the same versioned config.
- Persisted Favorite Lock startup: passed; when Favorite Lock is restored without an in-memory current channel, the first saved favorite candidate becomes the locked selection anchor.
- Favorite service smoke: passed with calibrated synthetic telemetry and a real Radio Garden candidate; `/control` returned `favorite=false id=207py2By name=80s80s - Bowie` after a toggle, and the service shut down cleanly afterward.
- Runtime status diagnostics: implemented and linted; authenticated-control-compatible `/status` exposes current mode, station/channel, provider, session, last sequence, resolved geo, and confidence, while the GUI polls it without blocking the UI.
- GUI now-playing summary: passed compilation/tests; runtime status is presented as station, country, city (honestly unknown when unavailable), mode, and confidence instead of exposing only raw JSON.
- GUI local logging: passed compilation; service stdout/stderr are appended to a per-user `roadwaves.log`, and the diagnostics panel exposes the path without any remote logging.
- Structured provider errors: passed compilation/tests; provider discovery and upstream MP3 failures are exposed through `/status`, CLI `service status`, and the GUI summary, then cleared after a successful live selection.
- Persisted-mode startup: passed compilation/tests; GUI startup sends the restored mode to the newly spawned service over authenticated control IPC before reporting the service as ready.
- GUI service startup race: hardened and linted; authenticated control requests retry bounded loopback connection attempts for up to two seconds while the freshly spawned service binds.
- Manual location override: passed compilation/tests; GUI latitude/longitude controls persist the override, send it through authenticated control IPC, reset it explicitly, and service selection/status use the override without claiming GPS accuracy.
- Versioned settings persistence: passed; per-user JSON stores mode, favorites/hidden-channel fields, manual location, ports, and startup preference; malformed settings are renamed to a timestamped `.corrupt-*` file and safe defaults are restored.
- CLI surface parity: passed; `config show`, `providers status`, `radio test`, `service run`, and transactional `uninstall-integration` now execute the same real configuration, Radio Garden, stream-probe, service, and integration paths as their legacy command equivalents.
- CLI service diagnostics: implemented and linted; `service status` reads the real loopback `/status` endpoint in human-readable or JSON form without requiring the GUI, and the end-to-end smoke invokes the JSON form.
- Doctor state coverage: implemented and linted; `doctor --json` now reports service reachability, RoadWaves-owned radio entries, and installed RoadWaves plugin targets in addition to discovery results, without modifying files.
- Doctor plugin-format safety: passed; target diagnostics recognize only the matching `.dll`/`.so` plugin for `win_x64`/`linux_x64` targets.
- Native Linux packaging definitions and explicit `build-deb.sh`/`build-rpm.sh` entrypoints added for DEB/RPM, desktop integration, icon, license/docs payload, and opt-in user service using `/usr/bin/roadwaves`; scripts are not executable on this Windows host.
- Proton profile classification tests: passed; Proton is treated as a Windows-profile/DLL path, not as a native Linux plugin path, and both `Documents` and `My Documents` prefix layouts are discovered.
- Official SCS SDK 1.15 archive inspected; native DLL compiled and export table verified for `scs_telemetry_init` and `scs_telemetry_shutdown`
- Native plugin ABI harness: passed; a disposable Windows harness loaded the release DLL, supplied SDK registration callbacks, invoked `truck.world.placement`, received valid ETS2 telemetry JSON on loopback with the injected coordinates, then exercised shutdown followed by reinitialization and verified a fresh session ID with sequence reset to zero.
- Windows plugin dependency audit: passed; the release DLL is statically linked against the MinGW C++/runtime libraries and its import table contains only Windows system APIs, so ETS2 does not need RoadWaves launcher runtime DLLs beside the game plugin.
- Rust service telemetry smoke test: UDP JSON packet received on `127.0.0.1:47927`; HTTP audio endpoint remained available on `127.0.0.1:47926`
- Service health smoke test: `/health` returned JSON HTTP 200; `/radio.mp3` returned HTTP 200, `audio/mpeg`, and the 12-byte MP3 fixture
- Repeatable service smoke script: passed; starts the release CLI, verifies health and MP3 bytes, sends UDP telemetry, applies authenticated manual location, confirms `/status`, rejects invalid-token and malformed Hide/Unhide control requests, and cleans up its own process.
- Audio listener hardening: passed compilation; loopback clients are handled concurrently with a four-client bound, request read timeouts, and a 503 response when capacity is exhausted.
- Audio failure recovery trigger: implemented and linted; upstream connection/status/codec/mid-stream read failures request the existing bounded Radio Garden rediscovery path, while the failed audio request remains an honest error rather than mislabeled bytes.
- Long-lived relay timeout: fixed and linted; upstream connection setup is bounded to ten seconds without imposing a total response timeout on continuous radio audio.
- Synthetic audio guard: passed smoke coverage; generated MP3 bytes require explicit hidden `--fixture` development mode, while normal service launches return HTTP 503 until a live Radio Garden stream is selected.
- No-fixture service smoke: passed; a normal release service launch returned HTTP 503 from `/radio.mp3`, exposed the no-live-stream reason through `provider_error`, and cleaned up its process.
- Audio bind safety: passed; the service accepts only numeric IPv4/IPv6 loopback addresses and rejects public, wildcard, and hostname binds.
- Dynamic station path: implemented and linted; calibrated telemetry triggers bounded nearby Radio Garden discovery, MP3 probing, weighted station selection, and updates the service relay URL without querying on every telemetry tick.
- Graceful service shutdown: passed compilation/tests; Ctrl-C sets cancellation state, the telemetry receiver exits on its timeout, and the nonblocking audio listener stops without killing unrelated processes.
- Versioned telemetry contract: passed; service rejects malformed, wrong-game, non-finite, and stale sequence packets, while accepting new sessions and valid ETS2 world positions.
- Native plugin telemetry smoke: passed through the release service; synthetic packets were accepted with sequence/session/world fields. Real ETS2 plugin loading remains pending.
- Native plugin build entrypoints: Linux `native/build-plugin.sh` produces `roadwaves_scs.so`; Windows `native/build-plugin.ps1` produces `roadwaves_scs.dll`. Neither Linux artifact was built on this host.

The MSVC linker/Visual Studio Build Tools is still not installed, so the MSVC target is not claimed as built. The local GNU target uses the checked-in LLVM-MinGW linker wrapper and the machine's Git SDK UCRT libraries. This host has no usable WSL distribution, Docker/Podman, Linux linker, or Linux runtime, so Fedora/Ubuntu artifacts cannot be built or installed here. The harness-loaded telemetry DLL is not equivalent to loading it inside the real ETS2 process. The installed ETS2 profile was inspected but not modified. Real in-game audio, native Linux packages, and Proton runtime behavior remain unverified. The Windows GUI and installer are built and smoke-tested locally, but have not been validated through a full live in-game session.
