#!/bin/sh
# Build the jl-speech helper (tools/macos-speech) and put it where Tauri's
# externalBin expects it: apps/desktop/binaries/jl-speech-<target triple>.
# Run by `cargo tauri build` on macOS (tauri.macos.conf.json).
set -e
cd "$(dirname "$0")/.."
swift build -c release --package-path tools/macos-speech
triple=$(rustc -vV | sed -n 's/^host: //p')
mkdir -p apps/desktop/binaries
cp tools/macos-speech/.build/release/jl-speech "apps/desktop/binaries/jl-speech-$triple"
