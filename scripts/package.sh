#!/bin/sh
set -e
cd "$(dirname "$0")/.."
triple=$(rustc -vV | sed -n 's/^host: //p')
cargo build --release -p fleetor-cli --bin fleet
mkdir -p src-tauri/binaries
cp target/release/fleet "src-tauri/binaries/fleet-$triple"
npm run tauri -- build --config src-tauri/tauri.bundle.conf.json
