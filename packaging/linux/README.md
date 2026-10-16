# Linux packaging

The RPM and DEB definitions target native x86_64 Linux builds and keep the optional user service disabled unless the user enables it. The package installs `/usr/bin/roadwaves`, `/usr/bin/roadwaves-gui`, the native plugin, desktop icon, license, and implementation report. From the repository root, run `bash packaging/linux/build-deb.sh` on Ubuntu or `bash packaging/linux/build-rpm.sh` on Fedora; each script builds the native Rust/plugin payload, stages it from the repository root, and writes the package to `dist/`. They are definitions only until built on Fedora/Ubuntu; no Linux artifact is claimed from this Windows workstation.

Proton is not a separate package architecture. The app must detect the active Steam compatdata prefix and install the Windows DLL and Windows-profile `live_streams.sii` entry there; that path still requires testing on a Linux Proton installation.
