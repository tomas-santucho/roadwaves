#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$root/target"
cxx=${CXX:-c++}
"$cxx" -std=c++17 -O2 -Wall -Wextra -fPIC -shared \
  -I"$root/native/scs_sdk/include" "$root/native/roadwaves_scs.cpp" \
  -o "$root/target/roadwaves_scs.so"
printf '%s\n' "$root/target/roadwaves_scs.so"
