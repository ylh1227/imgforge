#!/usr/bin/env bash
# 打包 macOS 图形界面应用（.app，双击即可使用）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(uname -m)"
case "$ARCH" in
  arm64) SUFFIX="macos-arm64" ;;
  x86_64) SUFFIX="macos-x64" ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

echo "==> Building ImgForge GUI v${VERSION} (${SUFFIX})..."
cargo build --release --features gui

BINARY=""
for candidate in "target/release/imgforge-app" "${CARGO_TARGET_DIR:-}/release/imgforge-app"; do
  if [ -n "$candidate" ] && [ -f "$candidate" ]; then
    BINARY="$candidate"
    break
  fi
done
if [ -z "$BINARY" ]; then
  TARGET_DIR="$(cargo metadata --format-version=1 --no-deps 2>/dev/null | grep -o '"target_directory":"[^"]*"' | head -1 | cut -d'"' -f4)"
  BINARY="${TARGET_DIR}/release/imgforge-app"
fi
if [ ! -f "$BINARY" ]; then
  echo "error: imgforge-app binary not found" >&2
  exit 1
fi

APP_NAME="ImgForge.app"
STAGE="$ROOT/dist/imgforge-${VERSION}-${SUFFIX}"
APP_PATH="$STAGE/$APP_NAME"
MACOS_DIR="$APP_PATH/Contents/MacOS"
RES_DIR="$APP_PATH/Contents/Resources"

rm -rf "$STAGE"
mkdir -p "$MACOS_DIR" "$RES_DIR"

cp "$BINARY" "$MACOS_DIR/imgforge-app"
chmod +x "$MACOS_DIR/imgforge-app"

sed "s/VERSION/${VERSION}/g" scripts/Info.plist.template > "$APP_PATH/Contents/Info.plist"

cp config.example.toml "$RES_DIR/"
cp scripts/QUICKSTART.txt "$RES_DIR/"
cp README.md "$RES_DIR/"
cp LICENSE "$RES_DIR/"

ARCHIVE="$ROOT/dist/imgforge-${VERSION}-${SUFFIX}-app.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGE" "$APP_NAME"

echo ""
echo "Done:"
echo "  App bundle: $APP_PATH"
echo "  Archive:    $ARCHIVE"
echo ""
echo "双击运行: open \"$APP_PATH\""
echo "若被拦截: xattr -dr com.apple.quarantine \"$APP_PATH\""
