#!/usr/bin/env bash
set -euo pipefail

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "native Ubuntu/Fedora packaging requires an x86_64 Linux host" >&2
  exit 2
fi
command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 2; }
command -v rpmbuild >/dev/null || { echo "rpmbuild is required" >&2; exit 2; }

version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)
[[ -n "$version" ]] || { echo "could not read workspace version" >&2; exit 2; }
cargo build --manifest-path "$repo/Cargo.toml" --workspace --release --target x86_64-unknown-linux-gnu
bash "$repo/native/build-plugin.sh"

stage=$(mktemp -d)
top=$(mktemp -d)
trap 'rm -rf "$stage" "$top"' EXIT
install -D -m 0755 "$repo/target/x86_64-unknown-linux-gnu/release/roadwaves" "$stage/roadwaves"
install -D -m 0755 "$repo/target/x86_64-unknown-linux-gnu/release/roadwaves-gui" "$stage/roadwaves-gui"
install -D -m 0755 "$repo/target/roadwaves_scs.so" "$stage/roadwaves_scs.so"
install -D -m 0644 "$repo/packaging/linux/roadwaves.desktop" "$stage/roadwaves.desktop"
install -D -m 0644 "$repo/packaging/linux/roadwaves.svg" "$stage/roadwaves.svg"
install -D -m 0644 "$repo/packaging/linux/roadwaves.service" "$stage/roadwaves.service"
install -D -m 0644 "$repo/README.md" "$stage/README.md"
install -D -m 0644 "$repo/LICENSE" "$stage/LICENSE"
install -D -m 0644 "$repo/docs/IMPLEMENTATION_REPORT.md" "$stage/IMPLEMENTATION_REPORT.md"
mkdir -p "$repo/dist" "$top/BUILD" "$top/RPMS" "$top/SRPMS" "$top/SPECS" "$top/SOURCES"
rpmbuild -bb "$repo/packaging/linux/roadwaves.spec" \
  --define "_topdir $top" \
  --define "_sourcedir $stage" \
  --define "rw_version $version" \
  --define "_rpmdir $repo/dist"
find "$repo/dist" -maxdepth 2 -type f -name 'roadwaves-*.rpm' -print
