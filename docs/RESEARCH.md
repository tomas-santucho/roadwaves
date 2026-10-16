# Research notes

Accessed 2026-10-15.

- The official [SCS Telemetry SDK page](https://modding.scssoft.com/wiki/Documentation/Engine/SDK/Telemetry) identifies Telemetry SDK 1.15 as stable and lists Windows, Linux, and macOS platforms. The official `scs_sdk_1_15.zip` was downloaded on 2026-10-15, its headers/examples were inspected, and its license was preserved under `native/scs_sdk_license.txt`.
- The Windows plugin uses the documented `scs_telemetry_init`/`scs_telemetry_shutdown` exports and `truck.world.placement` channel; its export table and loopback service receiver were verified locally.
- SCS documents `.sii` as a text/binary unit-serialized format and maintains a dedicated Game Radio documentation page. The current editor therefore preserves unrelated lines and writes a backup before changing the user file; native game acceptance is still pending.
- Radio Garden remains the only intended station discovery provider. Its community-observed endpoints are provisional; live responses must be treated as schema/access failures rather than silently replaced with another directory.
