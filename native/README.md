# SCS telemetry plugin

`roadwaves_scs.cpp` is a small ABI adapter against the official SCS Telemetry SDK 1.15 headers. The product core remains Rust; this native shim exists because the SCS SDK exposes a C++-layout initialization structure and game-loaded DLL entry points.

It registers `truck.world.placement` at each frame and sends only game coordinates as JSON lines over UDP to `127.0.0.1:47927`. It does not open a public listener or modify game memory.
Each telemetry initialization creates a fresh session ID and resets the sequence, so restarting ETS2 while the RoadWaves service remains alive is accepted as a new telemetry session.

The SDK headers are from the official `scs_sdk_1_15.zip`; see `scs_sdk_license.txt`. Build locally with the installed Git SDK:

```powershell
$env:PATH = "C:\git-sdk-64\ucrt64\bin;C:\git-sdk-64\usr\bin;" + $env:PATH
g++ -std=c++17 -O2 -Wall -Wextra -shared -static -static-libgcc -static-libstdc++ -Wl,--no-insert-timestamp -I native\scs_sdk native\roadwaves_scs.cpp -o target\roadwaves_scs.dll -lws2_32
```

The Windows build statically links the MinGW C++/runtime libraries so the game-loaded DLL does not depend on runtime files remaining beside the RoadWaves launcher. The system UCRT and Winsock APIs remain normal Windows dependencies.
The linker timestamp is omitted so repeated builds of the same source and toolchain produce the same plugin hash.

The DLL must not be copied into the live ETS2 installation until a user-approved integration step selects the matching game profile and verifies a backup can be restored.

On native Linux, build the matching shared object with `./native/build-plugin.sh`; it produces `target/roadwaves_scs.so`. Proton still requires the Windows build and installs `roadwaves_scs.dll` into the Proton game's `bin/win_x64/plugins` directory.
