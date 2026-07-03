#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

"$ROOT/Scripts/build_app.sh"

APP="$ROOT/.build/release/Mouseless.app"
DMG="$ROOT/.build/release/Mouseless.dmg"
STAGING="$ROOT/.build/dmg"

rm -rf "$STAGING" "$DMG"
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/Mouseless.app"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
  -volname "Mouseless" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$DMG"

echo "Built $DMG"
