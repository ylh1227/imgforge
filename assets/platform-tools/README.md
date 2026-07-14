# Bundled Android Platform Tools

Place the official Android SDK Platform-Tools files here before packaging releases:

- `assets/platform-tools/macos/adb`
- `assets/platform-tools/windows/adb.exe` plus the DLL files shipped with Platform-Tools
- `assets/platform-tools/linux/adb`

The packaging scripts copy the matching platform directory into the release bundle. At runtime,
imgforge resolves ADB in this order:

1. `mobile_pull.adb_path` / `--adb-path`
2. bundled `platform-tools/<platform>/adb`
3. `adb` from `PATH`, when `allow_path_fallback = true`

Use the official Android SDK Platform-Tools package from Google for these files.
