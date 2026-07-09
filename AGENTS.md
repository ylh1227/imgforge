# AGENTS.md

## Cursor Cloud specific instructions

`imgforge` is a single Rust project (Cargo workspace root at repo root) that ships two binaries plus a library:

- `imgforge` — CLI batch image converter (default features). Core commands documented in `README.md` (e.g. `imgforge -i <in> -o <out> -f webp`, `imgforge doctor`).
- `imgforge-app` — desktop GUI (egui/eframe), only built with `--features gui`.

Standard build/test/run commands live in `README.md` and `.github/workflows/ci.yml`; use those. Key ones:

- Build CLI: `cargo build`
- Test: `cargo test` (CI runs `cargo test --verbose`; the default suite is small — ~8 unit tests)
- Build GUI: `cargo build --features gui --bin imgforge-app`
- Run GUI (dev): `cargo run --features gui --bin imgforge-app` (or run the built `./target/debug/imgforge-app`)

### Non-obvious caveats

- **Toolchain**: the crate sets `rust-version = 1.85`. The base VM image may ship an older Rust (1.83); the environment has been updated to `stable` (>=1.85 required to build). The update script re-selects `stable`.
- **GUI needs a display**: a virtual X server is available at `DISPLAY=:1`. Always run the GUI with `DISPLAY=:1 ./target/debug/imgforge-app`. Without `DISPLAY` it will not start.
- **GUI CJK font rendering**: the GUI's Chinese labels render as empty squares ("tofu") in this environment because eframe's bundled default fonts do not include CJK glyphs. This is an app-level font limitation, not an environment bug — the app is fully functional; only the label glyphs are missing. Navigate by control position / rely on file output for verification.
- **video-review feature** requires system `ffmpeg` + `ffprobe` on PATH (installed in the snapshot). The GUI still starts without them, but video import/frame-extraction is disabled.
- **System libraries** for the GUI (already installed in the snapshot, not in the update script): OpenGL/mesa (`libgl1-mesa-dev`, `libglu1-mesa-dev`), and X/xkb runtime libs including `libxkbcommon-x11-0` and `libxcb-*` — without `libxkbcommon-x11` the GUI panics on startup with "Library libxkbcommon-x11.so could not be loaded".
- **Feature flags**: optional capabilities (avif, jpegxl, incremental, rename, thumbnails, watermark, review, video-review, data-extract) are gated behind Cargo features; see `[features]` in `Cargo.toml`. `gui` enables most of them.
