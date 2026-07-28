#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="${HOME}/sdk/flutter/bin:${PATH}"
export IMGFORGE_HOST="${IMGFORGE_HOST:-$ROOT/target/debug/imgforge-host}"
if [[ ! -x "$IMGFORGE_HOST" ]]; then
  echo "Building imgforge-host..."
  (cd "$ROOT" && cargo build --features host --bin imgforge-host)
  mkdir -p "$ROOT/target/debug"
  cp "${CARGO_TARGET_DIR:-$ROOT/target}/debug/imgforge-host" "$IMGFORGE_HOST" 2>/dev/null || true
fi
if ! xcrun --find xcodebuild >/dev/null 2>&1; then
  echo "缺少完整 Xcode（需要 /Applications/Xcode.app）。"
  echo "请从 App Store 安装 Xcode，然后执行："
  echo "  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
  echo "  sudo xcodebuild -license accept"
  exit 1
fi
cd "$(dirname "$0")"
exec flutter run -d macos
