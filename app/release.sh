#!/usr/bin/env bash
# Build, sign, notarize, staple, and package Incant.app for distribution.
#
# Prerequisites (one-time):
#   - Apple Developer Program membership ($99/yr)
#   - A "Developer ID Application" certificate in your login keychain
#   - A notary credential profile stored in keychain:
#       xcrun notarytool store-credentials incant-notary \
#         --apple-id "you@example.com" --team-id "TEAMID" --password "app-specific-pw"
#
# Configure via env:
#   SIGN_ID   = "Developer ID Application: Your Name (TEAMID)"
#   NOTARY    = "incant-notary"   (the stored profile name)
#   VERSION   = "0.1.0"           (defaults to project.yml value)
set -euo pipefail
cd "$(dirname "$0")"

SIGN_ID="${SIGN_ID:-}"
NOTARY="${NOTARY:-incant-notary}"
VERSION="${VERSION:-$(grep -m1 CFBundleShortVersionString project.yml | sed -E 's/.*"([^"]+)".*/\1/')}"
APP="build-dist/Build/Products/Release/Incant.app"
DIST="dist"

if [[ -z "$SIGN_ID" ]]; then
    echo "error: set SIGN_ID to your 'Developer ID Application' identity." >&2
    echo "       security find-identity -v -p codesigning   # to list them" >&2
    exit 1
fi

echo "› Building Release…"
xcodegen generate
xcodebuild -project Incant.xcodeproj -scheme Incant -configuration Release \
    -derivedDataPath build-dist \
    CODE_SIGN_IDENTITY="$SIGN_ID" CODE_SIGN_STYLE=Manual \
    OTHER_CODE_SIGN_FLAGS="--timestamp --options runtime" build

echo "› Signing with hardened runtime…"
codesign --force --deep --options runtime --timestamp --sign "$SIGN_ID" "$APP"
codesign --verify --strict --verbose=2 "$APP"

echo "› Packaging zip for notarization…"
mkdir -p "$DIST"
ZIP="$DIST/Incant-$VERSION.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "› Notarizing (this can take a few minutes)…"
xcrun notarytool submit "$ZIP" --keychain-profile "$NOTARY" --wait

echo "› Stapling…"
xcrun stapler staple "$APP"

echo "› Repackaging stapled app…"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

SHA=$(shasum -a 256 "$ZIP" | awk '{print $1}')
echo
echo "Done: $ZIP"
echo "sha256: $SHA"
echo "Update Casks/incant.rb with this version + sha256, and attach the zip to the GitHub release."
