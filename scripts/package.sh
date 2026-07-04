#!/usr/bin/env bash
# 本地打包 macOS 可分发压缩包（无需对方安装 Rust）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
FEATURES="incremental,rename,thumbnails,watermark,jpegxl"
ARCH="$(uname -m)"

case "$ARCH" in
  arm64) SUFFIX="macos-arm64" ;;
  x86_64) SUFFIX="macos-x64" ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

echo "==> Building imgforge v${VERSION} (${SUFFIX})..."
cargo build --release --features "$FEATURES"

BINARY=""
for candidate in "target/release/imgforge" "${CARGO_TARGET_DIR:-}/release/imgforge"; do
  if [ -n "$candidate" ] && [ -f "$candidate" ]; then
    BINARY="$candidate"
    break
  fi
done
if [ -z "$BINARY" ]; then
  TARGET_DIR="$(cargo metadata --format-version=1 --no-deps 2>/dev/null | grep -o '"target_directory":"[^"]*"' | head -1 | cut -d'"' -f4)"
  BINARY="${TARGET_DIR}/release/imgforge"
fi
if [ ! -f "$BINARY" ]; then
  echo "error: release binary not found" >&2
  exit 1
fi

STAGE="$ROOT/dist/imgforge-${VERSION}-${SUFFIX}"
rm -rf "$STAGE"
mkdir -p "$STAGE"

cp "$BINARY" "$STAGE/"
cp config.example.toml "$STAGE/"
cp scripts/QUICKSTART.txt "$STAGE/"
cp README.md "$STAGE/"
cp LICENSE "$STAGE/"

chmod +x "$STAGE/imgforge"

ARCHIVE="$ROOT/dist/imgforge-${VERSION}-${SUFFIX}.tar.gz"
tar -czf "$ARCHIVE" -C "$ROOT/dist" "imgforge-${VERSION}-${SUFFIX}"

echo ""
echo "Done: $ARCHIVE"
echo "解压后运行: ./imgforge -i ./photos -o ./output -f webp"
