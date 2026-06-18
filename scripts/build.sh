#!/bin/bash
# Build the gnirehtet Rust relay for the current platform.
# No extra dependencies needed.
set -e

echo "Building gnirehtet..."
cargo build --release --manifest-path "$(dirname "$0")/../relay-rust/Cargo.toml"
echo "Binary at relay-rust/target/release/gnirehtet"
