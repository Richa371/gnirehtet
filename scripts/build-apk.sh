#!/bin/bash
# Build the gnirehtet Android APK. Requires JDK 17+ and Android SDK.
set -e

cd "$(dirname "$0")/.."

# Auto-detect Android SDK if local.properties doesn't exist
if [ ! -f local.properties ]; then
    for sdk in "$HOME/Android/Sdk" /usr/lib/android-sdk /opt/android-sdk; do
        if [ -d "$sdk" ] && ( [ -d "$sdk/platforms" ] || [ -d "$sdk/build-tools" ] ); then
            echo "sdk.dir=$sdk" > local.properties
            echo "Android SDK found at $sdk"
            break
        fi
    done
fi

echo "Building APK..."
./gradlew :app:assembleRelease
echo "APK at app/build/outputs/apk/release/app-release-unsigned.apk"
