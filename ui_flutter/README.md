# ImgForge Flutter Shell

Desktop UI for ImgForge. Talks to `imgforge-host` over NDJSON JSON-RPC (stdio).

## Develop

```bash
# Build host
cargo build --features host --bin imgforge-host

# Run shell (expects host next to binary or via IMGFORGE_HOST)
cd ui_flutter
export IMGFORGE_HOST="$(pwd)/../target/debug/imgforge-host"
flutter run -d macos
```

## Layout

- `lib/host/` — RPC client + event stream
- `lib/pages/` — five modules (convert / review / video / extract / tasks)
- `lib/widgets/` — shared chrome
