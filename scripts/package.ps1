# 本地打包 Windows 可分发 zip（无需对方安装 Rust）
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$Version = (Select-String -Path Cargo.toml -Pattern '^version' | Select-Object -First 1).Line -replace '.*"(.*)".*', '$1'
$Features = "incremental,rename,thumbnails,watermark,jpegxl"
$Suffix = "windows-x64"

Write-Host "==> Building imgforge v$Version ($Suffix)..."
cargo build --release --features $Features

$Stage = Join-Path $Root "dist\imgforge-$Version-$Suffix"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Path $Stage -Force | Out-Null

Copy-Item "target\release\imgforge.exe" $Stage
Copy-Item "config.example.toml" $Stage
Copy-Item "scripts\QUICKSTART.txt" $Stage
Copy-Item "README.md" $Stage
Copy-Item "LICENSE" $Stage

$Archive = Join-Path $Root "dist\imgforge-$Version-$Suffix.zip"
if (Test-Path $Archive) { Remove-Item -Force $Archive }
Compress-Archive -Path "$Stage\*" -DestinationPath $Archive

Write-Host ""
Write-Host "Done: $Archive"
Write-Host "解压后运行: .\imgforge.exe -i .\photos -o .\output -f webp"
