#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="${HOME}/sdk/flutter/bin:${PATH}"
# 避免 Cursor/沙箱残留的 CARGO_TARGET_DIR 写到别处，导致 IMGFORGE_HOST 仍是旧二进制。
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
if [[ "$CARGO_TARGET_DIR" != "$ROOT/target"* ]]; then
  export CARGO_TARGET_DIR="$ROOT/target"
fi
export IMGFORGE_HOST="${IMGFORGE_HOST:-$ROOT/target/debug/imgforge-host}"

# API Key 优先系统钥匙串（设置页「写入钥匙串」）。
# 不再从 ~/.zshrc 拉明文进进程环境，避免 Key 出现在 process env。
# 若需临时调试，可自行 export IMGFORGE_VISION_API_KEY（仍仅作回退）。
if [[ -n "${IMGFORGE_VISION_API_KEY:-}" ]]; then
  echo "IMGFORGE_VISION_API_KEY: present in env (fallback; prefer Keychain)"
else
  echo "Vision API Key: use Keychain via 场景识别设置（不推荐写入 shell 明文）"
fi

echo "Building imgforge-host → $IMGFORGE_HOST"
(cd "$ROOT" && cargo build --features host --bin imgforge-host)
if [[ ! -x "$IMGFORGE_HOST" ]]; then
  echo "ERROR: imgforge-host missing at $IMGFORGE_HOST" >&2
  exit 1
fi

if ! xcrun --find xcodebuild >/dev/null 2>&1; then
  echo "缺少完整 Xcode（需要 /Applications/Xcode.app）。"
  echo "请从 App Store 安装 Xcode，然后执行："
  echo "  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
  echo "  sudo xcodebuild -license accept"
  exit 1
fi

# media_kit_video downloads mpv headers from GitHub during `pod install`.
# That often fails offline / behind slow GitHub; seed vendored headers + keep umbrella in sync.
seed_media_kit_mpv_headers() {
  local ui="$(cd "$(dirname "$0")" && pwd)"
  local src="$ui/tool/mpv_headers"
  local bundle="$ui/tool/mpv_headers_bundle.tar.gz"
  [[ -d "$src" ]] || return 0
  local pkg_cfg="$ui/.dart_tool/package_config.json"
  [[ -f "$pkg_cfg" ]] || return 0
  local video_root
  video_root="$(python3 - "$pkg_cfg" <<'PY'
import json, sys, pathlib
cfg = json.load(open(sys.argv[1]))
for p in cfg.get("packages", []):
    if p.get("name") == "media_kit_video":
        root = pathlib.Path(p["rootUri"].replace("file://", ""))
        if not root.is_absolute():
            root = (pathlib.Path(sys.argv[1]).parent / root).resolve()
        print(root)
        break
PY
)"
  [[ -n "$video_root" && -d "$video_root" ]] || return 0

  mkdir -p "$video_root/macos/Headers/mpv"
  cp -f "$src"/*.h "$video_root/macos/Headers/mpv/" 2>/dev/null || true

  # Prefer vendored tarball for Makefile so pod install never hits GitHub.
  if [[ -f "$bundle" ]]; then
    local cache="$video_root/common/darwin/.cache/headers"
    mkdir -p "$cache"
    cp -f "$bundle" "$cache/mpv.tar.gz.tmp"
    # Skip upstream sha when vendoring (Makefile may still be stock).
    if [[ -f "$video_root/common/darwin/Makefile" ]] && ! grep -q 'imgforge vendored mpv headers' "$video_root/common/darwin/Makefile"; then
      python3 - "$video_root/common/darwin/Makefile" "$bundle" <<'PY'
from pathlib import Path
import sys
mf, bundle = Path(sys.argv[1]), sys.argv[2]
text = mf.read_text()
needle = "\tcurl -L \\\n\t\thttps://github.com/mpv-player/mpv/archive/refs/tags/${MPV_HEADERS_VERSION}.tar.gz \\\n\t\to .cache/headers/mpv.tar.gz.tmp\n"
repl = f"\t# imgforge vendored mpv headers\n\tcp \"{bundle}\" .cache/headers/mpv.tar.gz.tmp\n"
if needle in text:
    text = text.replace(needle, repl)
    text = text.replace(
        "\tshasum -a 256 -c <<< '${MPV_HEADERS_SHA256SUM}  .cache/headers/mpv.tar.gz.tmp'\n",
        "\t# imgforge vendored mpv headers: skip upstream sha\n",
    )
    mf.write_text(text)
PY
    fi
    mv -f "$cache/mpv.tar.gz.tmp" "$cache/mpv-v0.36.0.tar.gz" 2>/dev/null || true
    ln -sfn mpv-v0.36.0.tar.gz "$cache/mpv.tar.gz"
  fi

  local ephemeral="$ui/macos/Flutter/ephemeral/.symlinks/plugins/media_kit_video/macos"
  if [[ -d "$ephemeral" ]]; then
    mkdir -p "$ephemeral/Headers/mpv"
    cp -f "$src"/*.h "$ephemeral/Headers/mpv/" 2>/dev/null || true
  fi

  local umbrella="$ui/macos/Pods/Target Support Files/media_kit_video/media_kit_video-umbrella.h"
  if [[ -f "$umbrella" ]] && ! grep -q 'client.h' "$umbrella"; then
    echo "Re-running pod install so media_kit_video umbrella imports mpv headers..."
    (cd "$ui/macos" && pod install >/dev/null)
  fi
  echo "Seeded media_kit_video mpv headers from tool/mpv_headers"
}
seed_media_kit_mpv_headers

cd "$(dirname "$0")"
exec flutter run -d macos
