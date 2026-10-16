#!/usr/bin/env bash
set -euo pipefail

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "native Ubuntu/Fedora packaging requires an x86_64 Linux host" >&2
  exit 2
fi
command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 2; }
command -v dpkg-deb >/dev/null || { echo "dpkg-deb is required" >&2; exit 2; }

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)
[[ -n "$version" ]] || { echo "could not read workspace version" >&2; exit 2; }
cargo build --manifest-path "$repo/Cargo.toml" --workspace --release --target x86_64-unknown-linux-gnu
bash "$repo/native/build-plugin.sh"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
install -D -m 0755 "$repo/target/x86_64-unknown-linux-gnu/release/roadwaves" "$stage/usr/bin/roadwaves"
install -D -m 0755 "$repo/target/x86_64-unknown-linux-gnu/release/roadwaves-gui" "$stage/usr/bin/roadwaves-gui"
install -D -m 0755 "$repo/target/roadwaves_scs.so" "$stage/usr/lib/roadwaves/plugins/roadwaves_scs.so"
install -D -m 0644 "$repo/packaging/linux/roadwaves.desktop" "$stage/usr/share/applications/roadwaves.desktop"
install -D -m 0644 "$repo/packaging/linux/roadwaves.svg" "$stage/usr/share/icons/hicolor/scalable/apps/roadwaves.svg"
install -D -m 0644 "$repo/packaging/linux/roadwaves.service" "$stage/usr/lib/systemd/user/roadwaves.service"
install -D -m 0644 "$repo/README.md" "$stage/usr/share/doc/roadwaves/README.md"
install -D -m 0644 "$repo/LICENSE" "$stage/usr/share/doc/roadwaves/LICENSE"
install -D -m 0644 "$repo/docs/IMPLEMENTATION_REPORT.md" "$stage/usr/share/doc/roadwaves/IMPLEMENTATION_REPORT.md"
install -D -m 0644 "$repo/packaging/linux/debian/control.runtime" "$stage/DEBIAN/control"
sed -i "s/@VERSION@/$version/" "$stage/DEBIAN/control"

mkdir -p "$repo/dist"
dpkg-deb --build --root-owner-group "$stage" "$repo/dist/roadwaves_${version}_amd64.deb"
echo "$repo/dist/roadwaves_${version}_amd64.deb"
