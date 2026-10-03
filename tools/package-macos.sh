#!/usr/bin/env bash
# Build universal mhdn.app from existing per-target release binaries (no cargo build).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BIN_NAME="mhdn"
ARM_BIN="$ROOT/target/aarch64-apple-darwin/release/$BIN_NAME"
X64_BIN="$ROOT/target/x86_64-apple-darwin/release/$BIN_NAME"
OUT_APP="$ROOT/dist/mhdn.app"
MACOS_DIR="$OUT_APP/Contents/MacOS"
RESOURCES_DIR="$OUT_APP/Contents/Resources"
PLIST="$OUT_APP/Contents/Info.plist"
ICNS_SRC="$ROOT/assets/mhdn.icns"

if [[ ! -f "$ARM_BIN" ]]; then
  echo "error: missing $ARM_BIN (run: cargo build -p mhdn-app --release --target aarch64-apple-darwin)" >&2
  exit 1
fi
if [[ ! -f "$X64_BIN" ]]; then
  echo "error: missing $X64_BIN (run: cargo build -p mhdn-app --release --target x86_64-apple-darwin)" >&2
  exit 1
fi
if [[ ! -f "$ICNS_SRC" ]]; then
  echo "error: missing $ICNS_SRC" >&2
  exit 1
fi

rm -rf "$OUT_APP"
mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"
cp "$ICNS_SRC" "$RESOURCES_DIR/mhdn.icns"

lipo -create -output "$MACOS_DIR/$BIN_NAME" "$ARM_BIN" "$X64_BIN"
chmod +x "$MACOS_DIR/$BIN_NAME"

cat >"$PLIST" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>mhdn</string>
  <key>CFBundleExecutable</key>
  <string>mhdn</string>
  <key>CFBundleIdentifier</key>
  <string>com.esrgan.mhdn</string>
  <key>CFBundleIconFile</key>
  <string>mhdn</string>
  <key>LSUIElement</key>
  <true/>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

echo "Created $OUT_APP"
