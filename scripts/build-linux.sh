#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
cargo build --locked --release --bin rustclipsync-agent
cmake -S ui/linux -B build/linux -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build/linux --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-2}"
if [[ "${1:-}" == "--stage" ]]; then
    stage="$root/dist/linux-root"
    rm -rf -- "$stage"
    DESTDIR="$stage" cmake --install build/linux
fi
