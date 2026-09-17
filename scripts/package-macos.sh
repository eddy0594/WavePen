#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) WavePen.app and wraps it in a DMG
# with an Applications shortcut. Without an Apple Developer account the app is
# not notarized, so recipients must allow it once in Privacy & Security.
#
# Signing: when the "WavePen Self-Signed Code Signing" identity exists in the
# keychain it is used, which keeps the code signature's designated requirement
# stable across builds so macOS keeps Accessibility, Microphone, and keychain
# grants after updates. Otherwise the app falls back to ad-hoc signing.
set -euo pipefail

SELF_SIGNED_IDENTITY="WavePen Self-Signed Code Signing"
if security find-identity -p codesigning | grep -q "\"$SELF_SIGNED_IDENTITY\""; then
  export APPLE_SIGNING_IDENTITY="$SELF_SIGNED_IDENTITY"
  echo "Signing with: $SELF_SIGNED_IDENTITY"
else
  echo "Self-signed identity not found; using ad-hoc signing"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
APP="$BUNDLE_DIR/macos/WavePen.app"
OUT_DIR="dist-release"
DMG="$OUT_DIR/WavePen_${VERSION}_universal.dmg"

npm run tauri build -- --target universal-apple-darwin --bundles app

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
cp "docs/WavePen-安裝說明.md" "$STAGING/安裝說明.md"
cp LICENSE "$STAGING/LICENSE.txt"

mkdir -p "$OUT_DIR"
rm -f "$DMG"
hdiutil create -volname "WavePen" -srcfolder "$STAGING" -fs HFS+ -format UDZO -ov "$DMG"
echo "Created $DMG"
