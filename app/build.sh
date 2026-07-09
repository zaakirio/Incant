#!/usr/bin/env bash
# Build Incant.app from source. Requires Xcode command-line tools and
# XcodeGen (brew install xcodegen). No need to open the Xcode IDE.
set -euo pipefail
cd "$(dirname "$0")"

xcodegen generate
xcodebuild -project Incant.xcodeproj -scheme Incant -configuration Release \
    -derivedDataPath build-dist build

APP="build-dist/Build/Products/Release/Incant.app"
echo "Built $APP"

if [[ "${1:-}" == "--dist" ]]; then
    mkdir -p dist
    rm -f dist/Incant.zip
    ditto -c -k --keepParent "$APP" dist/Incant.zip
    echo "Packaged dist/Incant.zip"
fi

if [[ "${1:-}" == "--run" ]]; then
    pkill -x Incant 2>/dev/null || true
    open "$APP"
fi
