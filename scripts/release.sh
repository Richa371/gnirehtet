#!/bin/bash
# Package a release for the current platform: gnirehtet binary + adb + gnirehtet.apk.
# Usage: ./scripts/release.sh [version]
set -e

SCRIPT_DIR="$(dirname "$0")"
cd "$SCRIPT_DIR/.."

DIST=dist
version="${1:-$(git describe --tags --always 2>/dev/null || echo "snapshot")}"

# Detect platform
case "$(uname -s)" in
    Linux)  PLATFORM="linux";  BINARY="gnirehtet" ;;
    Darwin) PLATFORM="darwin"; BINARY="gnirehtet" ;;
    MINGW*|MSYS*) PLATFORM="windows"; BINARY="gnirehtet.exe" ;;
esac
case "$(uname -m)" in
    x86_64|amd64) ARCH="64" ;;
    aarch64|arm64) ARCH="arm64" ;;
esac
DIR_NAME="gnirehtet-${PLATFORM}${ARCH}"

echo "Building for ${PLATFORM}${ARCH}..."

# Build Rust relay
cargo build --release --manifest-path relay-rust/Cargo.toml

# Build APK (optional — may fail without Android SDK)
if command -v javac &>/dev/null; then
    bash scripts/build-apk.sh 2>/dev/null || echo "APK build skipped (no Android SDK?)"
fi

# Bundle
mkdir -p "$DIST/$DIR_NAME"
cp "relay-rust/target/release/$BINARY" "$DIST/$DIR_NAME/gnirehtet$([[ $PLATFORM == "windows" ]] && echo ".exe" || true)"
[ -f app/build/outputs/apk/release/app-release-unsigned.apk ] && \
    cp app/build/outputs/apk/release/app-release-unsigned.apk "$DIST/$DIR_NAME/gnirehtet.apk"

# Download and bundle ADB
ADB_URL="https://dl.google.com/android/repository/platform-tools-latest-${PLATFORM}.zip"
echo "Downloading ADB..."
curl -fLo /tmp/adb.zip "$ADB_URL"
unzip -o -j /tmp/adb.zip "platform-tools/adb*" -d "$DIST/$DIR_NAME" 2>/dev/null
rm /tmp/adb.zip
chmod +x "$DIST/$DIR_NAME/adb" 2>/dev/null || true

# Package
cd "$DIST"
rm -f "${DIR_NAME}-${version}.zip"
zip -r "${DIR_NAME}-${version}.zip" "$DIR_NAME"
rm -rf "$DIR_NAME"
sha256sum "${DIR_NAME}-${version}.zip" > SHA256SUMS.txt 2>/dev/null || true
echo ""
echo "Created: $DIST/${DIR_NAME}-${version}.zip"
