# imgforge

跨平台、高性能批量图片格式转换工具。提供 **图形界面（双击即用）** 与 **命令行** 两种模式，采用 tokio 异步 IO + rayon CPU 并行架构。

## 图形界面版（推荐）

无需命令行，像普通 App 一样使用：选文件夹 → 选格式 → 点「开始转换」。

| 平台 | 下载文件 | 使用方式 |
|------|----------|----------|
| **Windows** | `imgforge-*-windows-x64-app.zip` | 解压后双击 `ImgForge.exe` |
| **macOS (M 芯片)** | `imgforge-*-macos-arm64-app.tar.gz` | 解压后双击 `ImgForge.app` |
| **macOS (Intel)** | `imgforge-*-macos-x64-app.tar.gz` | 解压后双击 `ImgForge.app` |

### 界面功能

- 浏览选择输入 / 输出文件夹（支持拖入文件夹）
- 选择目标格式、质量、是否保留目录结构
- 实时进度条与日志
- 完成后一键打开输出文件夹

### macOS 首次打开

若提示「无法验证开发者」：

```bash
xattr -dr com.apple.quarantine /path/to/ImgForge.app
```

或在「系统设置 → 隐私与安全性」中允许打开。

### 本地打包图形界面

```bash
# macOS → 生成 ImgForge.app
chmod +x scripts/package-gui.sh
./scripts/package-gui.sh
```

```powershell
# Windows → 生成 ImgForge.exe
.\scripts\package-gui.ps1
```

```bash
# 开发时直接运行 GUI
cargo run --release --features gui --bin imgforge-app
```

## 特性

- 主流格式互转：JPEG / PNG / WebP / BMP / TIFF / GIF
- 批量递归转换，保留目录层级结构
- 可配置输出质量，支持无损 PNG/WebP
- 自适应多线程并发（默认匹配 CPU 核心数）
- 原子写入（`.tmp` + `rename`），防止崩溃损坏文件
- 实时进度条与最终统计报告
- 单文件失败隔离，不中断整体任务
- dry-run 预览模式
- 等比缩放、裁剪、旋转翻转
- EXIF 元数据保留/剥离
- 文件后缀与大小过滤
- 内置预设：web / minimal / print
- Ctrl+C 优雅退出
- TOML 配置文件支持

### P2 扩展（按需 feature 开启）

- **AVIF** 编码（`ravif` 纯 Rust）；解码需 `avif-decode`（需 cmake/libaom）
- **JPEG XL** 编解码（`jxl-oxide` + `zune-jpegxl`）
- **水印**：图片叠加 + 文字水印（`ab_glyph`，文字需指定字体）
- **增量处理**：SHA-256 断点续跑（`--incremental`）
- **重命名模板**：`{stem}` `{name}` `{ext}` `{dir}` `{index}` 等占位符
- **多尺寸缩略图**：一次生成多个规格（如 `256,512x384`）
- **libvips 后端**：占位实现，当前回退原生后端

## 下载预编译版本

### 图形界面版（推荐普通用户）

见上文 [图形界面版](#图形界面版推荐)。

### 命令行版（高级用户）

无需安装 Rust，解压即可使用。发布包内含 `imgforge` 可执行文件、`config.example.toml` 和快速上手指南。

| 平台 | 文件 | 说明 |
|------|------|------|
| Windows x64 | `imgforge-*-windows-x64.zip` | 解压后运行 `imgforge.exe` |
| macOS Apple Silicon | `imgforge-*-macos-arm64.tar.gz` | M 系列芯片 |
| macOS Intel | `imgforge-*-macos-x64.tar.gz` | Intel 芯片 |

在 [GitHub Releases](https://github.com/ylh1227/imgforge/releases) 页面下载对应版本。

### Windows 快速使用

```powershell
# 解压 zip 后，在 PowerShell 中：
.\imgforge.exe -i .\photos -o .\output -f webp
.\imgforge.exe doctor
```

### macOS 快速使用

```bash
# 解压 tar.gz 后，在终端中：
chmod +x imgforge
xattr -dr com.apple.quarantine ./imgforge   # 首次运行若被拦截，执行此行
./imgforge -i ./photos -o ./output -f webp
./imgforge doctor
```

预编译包已内置常用扩展：增量处理、重命名、缩略图、水印、AVIF 编码、JPEG XL。

### 本地自行打包

若需在本机构建可分发的压缩包：

```bash
# macOS
chmod +x scripts/package.sh
./scripts/package.sh
# 输出: dist/imgforge-<version>-macos-arm64.tar.gz（或 macos-x64）
```

```powershell
# Windows
.\scripts\package.ps1
# 输出: dist\imgforge-<version>-windows-x64.zip
```

### 发布新版本（维护者）

```bash
git tag v0.1.0
git push origin v0.1.0
```

推送 `v*` 标签后，GitHub Actions 会自动构建 **图形界面版 + 命令行版**（Windows / macOS）并创建 Release。

## 编译

支持 **Windows / macOS / Linux**，需要 Rust 1.85+（2021 edition）。

### Windows

1. 安装 [Rust](https://rustup.rs)（运行 `rustup-init.exe`，默认选项即可）
2. 若编译报链接错误，安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」
3. 重新打开 PowerShell 后编译：

```powershell
cd imgforge
cargo build --release
```

可执行文件位于 `.\target\release\imgforge.exe`。建议将其加入 PATH，或复制到已在 PATH 的目录中，之后可直接使用 `imgforge` 命令。

> **Windows 编译提示**
> - 路径支持 `.\photos` 或 `C:\photos` 形式；输入与输出可在不同盘符（如 `C:\` → `D:\`）
> - 建议在 Win10+ 启用 [长路径支持](https://learn.microsoft.com/windows/win32/fileio/maximum-file-path-limitation)
> - AVIF 解码（`avif-decode`）需额外安装 [cmake](https://cmake.org) 与 Visual Studio 构建工具
> - libvips（`vips` feature）需通过 [vcpkg](https://vcpkg.io) 等方式单独安装

### macOS / Linux

```bash
cd imgforge
cargo build --release
```

### 可选 Feature

```bash
# 增量处理（基于 SHA-256 哈希）
cargo build --release --features incremental

# AVIF 编码（纯 Rust）
cargo build --release --features avif

# AVIF 解码（额外需要 cmake + libaom）
cargo build --release --features "avif,avif-decode"

# JPEG XL 编解码
cargo build --release --features jpegxl

# 水印 / 重命名 / 缩略图
cargo build --release --features watermark
cargo build --release --features rename
cargo build --release --features thumbnails

# 全量 P2 功能
cargo build --release --features "incremental,rename,thumbnails,watermark,avif,jpegxl,vips"

# libvips 后端（需系统安装 libvips，如 `brew install vips`）
cargo build --release --features vips
```

> **注意**：`--features vips` 需要系统已安装 libvips 开发库；运行时若初始化失败会自动回退原生后端。

## Windows 使用方法

以下示例均在 **PowerShell** 中运行。PowerShell 多行命令用反引号 `` ` `` 续行；路径含空格时需加引号。

### 快速开始

```powershell
# 将当前目录下 photos 文件夹中的图片全部转为 webp
.\target\release\imgforge.exe -i .\photos -o .\output -f webp

# 使用绝对路径（输入输出可在不同盘符）
.\target\release\imgforge.exe -i C:\Users\你\Pictures -o D:\导出 -f jpg

# 含空格的路径需加引号
.\target\release\imgforge.exe -i "D:\我的 照片" -o "D:\导出结果" -f webp
```

### 常用参数

| 短参数 | 长参数 | 说明 |
|--------|--------|------|
| `-i` | `--input` | 输入目录 |
| `-o` | `--output` | 输出目录 |
| `-f` | `--format` | 目标格式：`jpg` `png` `webp` `bmp` `tiff` `gif` |
| `-q` | `--quality` | 输出质量 1–100 |
| `-j` | `--concurrency` | 并发线程数 |
| | `--overwrite` | 覆盖已存在的输出文件 |
| | `--flat` | 扁平输出，不保留目录结构 |
| | `--dry-run` | 预览模式，不实际写入 |
| | `--width` / `--height` | 缩放尺寸 |
| | `--strip-metadata` | 剥离 EXIF 元数据 |
| `-c` | `--config` | 指定 TOML 配置文件 |
| `-v` | `--verbose` | 详细日志 |

也可通过环境变量设置（在 PowerShell 当前会话中）：

```powershell
$env:IMGFORGE_INPUT = "C:\photos"
$env:IMGFORGE_OUTPUT = "D:\output"
$env:IMGFORGE_FORMAT = "webp"
$env:IMGFORGE_QUALITY = "85"
imgforge -i $env:IMGFORGE_INPUT -o $env:IMGFORGE_OUTPUT -f webp
```

### 基础批量转换

```powershell
# 指定质量与并发数
imgforge -i .\photos -o .\output -f jpg -q 90 -j 8

# 保留子目录结构（默认开启）
imgforge -i .\photos -o .\output -f png --preserve-structure

# 覆盖已存在文件
imgforge -i .\photos -o .\output -f webp --overwrite
```

### 缩放、预设与过滤

```powershell
# 缩放到 1920x1080 并锐化
imgforge -i .\photos -o .\output -f webp --width 1920 --height 1080 --sharpen 0.5

# 使用内置预设
imgforge -i .\photos -o .\output --preset web      # Web 优化
imgforge -i .\photos -o .\output --preset minimal  # 最小体积
imgforge -i .\photos -o .\output --preset print   # 高清打印

# 仅处理 jpg/png，大于 10KB
imgforge -i .\photos -o .\output -f webp --extensions jpg,png --min-size 10240

# 预览，不写入文件
imgforge -i .\photos -o .\output -f webp --dry-run
```

### 配置文件

```powershell
Copy-Item config.example.toml imgforge.toml
# 编辑 imgforge.toml 后运行
imgforge -c imgforge.toml -i .\photos -o .\output
```

文字水印字体路径示例（`config.example.toml` 中）：

```toml
# font_path = "C:\\Windows\\Fonts\\msyh.ttc"    # 微软雅黑（支持中文）
# font_path = "C:\\Windows\\Fonts\\arial.ttf"   # Arial（仅英文）
```

### P2 扩展功能（需重新编译）

```powershell
# 增量处理、重命名、缩略图、水印
cargo build --release --features "incremental,rename,thumbnails,watermark"

# 增量处理：跳过未变化的文件
imgforge -i .\photos -o .\output -f webp --incremental

# 重命名模板
imgforge -i .\photos -o .\output -f webp --rename-template "{stem}_optimized"

# 多尺寸缩略图
imgforge -i .\photos -o .\output -f webp --thumbnail-sizes 128,512x384

# 图片水印
imgforge -i .\photos -o .\output -f webp --watermark-image .\logo.png

# 文字水印（中文请用微软雅黑等中文字体）
imgforge -i .\photos -o .\output -f webp `
  --watermark-text "© 2026" `
  --watermark-font "C:\Windows\Fonts\msyh.ttc"

# 旋转 / 缩放模式
imgforge -i .\photos -o .\output -f webp --transform rotate90
imgforge -i .\photos -o .\output -f webp --width 800 --resize-mode fill
```

### 环境诊断

```powershell
imgforge doctor
```

检查运行时平台、已启用 feature 和后端状态。

### PowerShell 命令补全

```powershell
# 生成补全脚本并写入 PowerShell 配置文件
imgforge completions powershell | Out-File -Append $PROFILE -Encoding utf8
```

重新打开 PowerShell 后，输入 `imgforge` 按 `Tab` 即可自动补全。

## 使用示例

> 以下示例为 macOS / Linux（bash）写法。Windows 用户请参见上文 [Windows 使用方法](#windows-使用方法)。

### 基础批量转换

```bash
# 将 ./photos 下所有图片转为 WebP，输出到 ./output
imgforge -i ./photos -o ./output -f webp

# 指定质量与并发数
imgforge -i ./photos -o ./output -f jpg --quality 90 -j 8
```

### 保留目录结构

```bash
imgforge -i ./photos -o ./output -f png --preserve-structure
```

### 缩放与画质调整

```bash
imgforge -i ./photos -o ./output -f webp --width 1920 --height 1080 --sharpen 0.5
```

### 使用预设

```bash
# Web 优化：WebP 82 质量，最大 1920x1080，剥离元数据
imgforge -i ./photos -o ./output --preset web

# 最小体积
imgforge -i ./photos -o ./output --preset minimal

# 高清打印
imgforge -i ./photos -o ./output --preset print
```

### 过滤与预览

```bash
# 仅处理 jpg/png，大于 10KB
imgforge -i ./photos -o ./output -f webp --extensions jpg,png --min-size 10240

# dry-run 预览，不写入文件
imgforge -i ./photos -o ./output -f webp --dry-run
```

### 配置文件

```bash
cp config.example.toml imgforge.toml
imgforge -c imgforge.toml -i ./photos -o ./output
```

### Shell 补全

```bash
imgforge completions zsh > ~/.zsh/completions/_imgforge
```

### P2 扩展示例

```bash
# 增量处理：跳过未变化的文件
cargo run --release --features incremental -- \
  -i ./photos -o ./output -f webp --incremental

# 重命名模板
cargo run --release --features rename -- \
  -i ./photos -o ./output -f webp --rename-template "{stem}_optimized"

# 多尺寸缩略图（每张原图生成 128px 和 512x384 两个版本）
cargo run --release --features thumbnails -- \
  -i ./photos -o ./output -f webp --thumbnail-sizes 128,512x384

# 图片水印
cargo run --release --features watermark -- \
  -i ./photos -o ./output -f webp --watermark-image ./logo.png

# 文字水印（需指定字体文件）
cargo run --release --features watermark -- \
  -i ./photos -o ./output -f webp \
  --watermark-text "© 2026" --watermark-font /System/Library/Fonts/Helvetica.ttc

# AVIF 输出
cargo run --release --features avif -- \
  -i ./photos -o ./output -f avif --quality 80

# 旋转/翻转
imgforge -i ./photos -o ./output -f webp --transform rotate90

# 缩放模式
imgforge -i ./photos -o ./output -f webp --width 800 --resize-mode fill

# 环境诊断
imgforge doctor
```

## 配置优先级

1. 命令行参数
2. 环境变量（`IMGFORGE_*`）
3. TOML 配置文件
4. 内置默认值

## 架构

```
CLI / Config
     │
     ▼
 Scanner (jwalk) ──► Task List
     │
     ▼
 Executor (tokio + rayon)
     │  ├─ async read
     │  ├─ pipeline (CPU)
     │  └─ atomic write
     ▼
 Progress + Report
```

## License

MIT
