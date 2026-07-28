# ImgForge Flutter Shell

Desktop UI for ImgForge. Talks to `imgforge-host` over NDJSON JSON-RPC (stdio).

## Develop

```bash
# 1) Build host
cargo build --features host --bin imgforge-host

# 2) Run shell (macOS needs full Xcode.app, not just CLT)
./ui_flutter/run_macos.sh
# or:
cd ui_flutter
export IMGFORGE_HOST="$(pwd)/../target/debug/imgforge-host"
flutter run -d macos
```

若报 `unable to find utility "xcodebuild"`：从 App Store 安装 **Xcode**，再执行：

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
```

## Layout

- `lib/host/` — RPC client + event stream
- `lib/pages/` — five modules (convert / review / video / extract / tasks)
- `lib/widgets/` — shared chrome
