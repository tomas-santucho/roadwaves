# Development

Required on Windows: stable Rust, Cargo, Git, and a C compiler for crates that need native code. This machine has the Git for Windows SDK at `C:\git-sdk-64` and the checked-in LLVM-MinGW bundle under `.tools-llvm`; the workspace linker wrapper uses LLVM-MinGW with the SDK's UCRT libraries because Visual Studio's MSVC linker is not installed.

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release

# Local fallback target on this machine
rustup target add x86_64-pc-windows-gnu
cargo test --workspace --target x86_64-pc-windows-gnu
cargo build --workspace --release --target x86_64-pc-windows-gnu

# Local GNU target checks use the bundled linker wrapper; include its tools in PATH.
$env:PATH = "C:\path\to\roadwaves\.tools-llvm\llvm-mingw-20260908-ucrt-x86_64\bin;" + $env:PATH
cargo +stable-x86_64-pc-windows-gnu test --workspace --target x86_64-pc-windows-gnu -j 1
cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings

# Service and synthetic telemetry smoke test
.\target\x86_64-pc-windows-gnu\release\roadwaves.exe serve --fixture
.\target\x86_64-pc-windows-gnu\release\roadwaves.exe simulate --route tests\fixtures\routes\berlin-to-warsaw.json --speed 10

# End-to-end local service smoke test. Uses only loopback and a temporary process;
# it does not edit the installed ETS2 profile.
powershell -ExecutionPolicy Bypass -File .\tests\service-smoke.ps1
powershell -ExecutionPolicy Bypass -File .\tests\service-no-fixture-smoke.ps1
```

CI is intentionally omitted for this local build. This Windows host has no usable WSL distribution, Docker/Podman, Linux linker, or Linux runtime; Fedora/Ubuntu package builds therefore remain unrun. The native ABI harness validates the DLL registration and loopback packet path, but real ETS2 audio, in-game SCS SDK loading, Proton, Fedora, and Ubuntu require platform-specific verification and are not claimed as complete by the current build.
