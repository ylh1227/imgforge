# 打包 Windows 图形界面应用（解压后双击 ImgForge.exe）
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'
$Suffix = "windows-x64-app"

Write-Host "==> Building ImgForge GUI v$Version..."
cargo build --release --features gui

$Binary = "target\release\imgforge-app.exe"
if (-not (Test-Path $Binary)) {
  throw "imgforge-app.exe not found at $Binary"
}

$Stage = Join-Path $Root "dist\imgforge-$Version-$Suffix"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Path $Stage -Force | Out-Null

Copy-Item $Binary (Join-Path $Stage "ImgForge.exe")
Copy-Item "config.example.toml" $Stage
Copy-Item "scripts\QUICKSTART.txt" $Stage
Copy-Item "README.md" $Stage
Copy-Item "LICENSE" $Stage

$PlatformTools = Join-Path $Root "assets\platform-tools\windows"
if (Test-Path $PlatformTools) {
  $Dest = Join-Path $Stage "platform-tools\windows"
  New-Item -ItemType Directory -Path $Dest -Force | Out-Null
  Copy-Item "$PlatformTools\*" $Dest -Recurse -Force
} else {
  Write-Host "note: bundled adb skipped; missing $PlatformTools"
}

$Archive = Join-Path $Root "dist\imgforge-$Version-$Suffix.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path "$Stage\*" -DestinationPath $Archive

Write-Host ""
Write-Host "Done: $Archive"
Write-Host "解压后双击 ImgForge.exe 即可使用"
