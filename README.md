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
- **图片评审** Tab：批次导入、状态/标签/标注、多图对比、导出 CSV/JSON，可与转换队列联动
- **数据提取** Tab：解析 Imatest 测试结果（13 类模块），汇总/对比/阈值评估，导出 CSV/JSON/HTML；可选 OCR 识别截图结果（需系统安装 `tesseract`）
- **任务中心** Tab：转换历史、失败重试、各模块操作日志
- **视频评审** Tab（需系统安装 `ffmpeg` / `ffprobe`）：
  - 从文件夹导入 mp4/mov/mkv/webm/avi/m4v
  - ffprobe 读取时长、分辨率、编码等元数据
  - 时间轴抽帧预览（非连续播放）；属性面板置于时间轴上方，时间轴占满主区域宽度
  - 2–6 路视频同步时间点对比（2 路并排，3+ 宫格）
  - **列表 / 卡片双视图**：列表模式高密度浏览 + 悬停预览；卡片模式展示封面与关键元数据
  - **多宫格拼接导出**：将当前对比选择按统一时间点抽帧，导出 PNG contact sheet
  - **对比拼接视频导出**：从当前时间点起，将 2–6 路视频按宫格布局合成 MP4；支持**高质量**（CRF 17）与**无损**（CRF 0）两种模式
  - 状态、标签、时间点标记、片段备注（含常用问题模板）
  - 多选批量更新状态、追加备注、应用标签
  - 偏移校准（`offset_ms`）对齐不同起点素材
  - 导出 CSV / JSON 报告（含标记/片段详情）
  - 抽帧缓存目录：`~/.imgforge/video_frames/`（可查看统计并清理）

#### 视频浏览：列表与卡片模式

左侧「视频列表」顶部可切换：

| 模式 | 适用场景 | 功能 |
|------|----------|------|
| **列表** | 快速浏览、批量勾选 | 密集行布局；悬停 200ms 后弹出当前时间点预览帧 |
| **卡片** | 视觉识别、查看元数据 | 封面网格；显示时长、分辨率、fps、编码、偏移与标签 |

列表模式悬停预览使用异步抽帧（320px 宽），纹理缓存上限 64 条，避免快速扫过列表时阻塞 UI。

#### 视频评审工作流（宫格导出）

1. 导入视频文件夹，在左侧勾选 2–6 个需要对比的视频（列表或卡片模式均可）
2. 点击「对比」进入多视频模式，拖动时间轴到目标时间点
3. 点击「导出宫格」或切到「导出」Tab →「导出当前对比宫格…」
4. 导出 Tab 会预览行列布局（如 2×2、2×3）与输出尺寸；成功后显示绿色确认
5. 需要视频片段时：在「导出」Tab 调整片段时长 →「导出对比拼接视频…」，保存 MP4
6. 保存 PNG / MP4，可直接用于外部沟通；CSV/JSON 报告包含完整评审元数据

导出失败时会明确提示：未选够视频、ffmpeg 不可用、抽帧失败或保存失败。

#### 视频评审依赖

```bash
# macOS (Homebrew)
brew install ffmpeg

# Windows (winget)
winget install Gyan.FFmpeg
```

未安装时 App 可正常启动，顶部会提示；导入与抽帧功能不可用。

#### 数据提取（Imatest）

GUI「数据提取」Tab 面向相机/镜头测试工作流：

1. 选择包含 Imatest 导出结果的目录（CSV / JSON / TXT）
2. 自动识别 13 类模块并汇总关键指标
3. 可做批次对比、阈值评估与洞察报告
4. 导出 CSV / JSON / HTML

若结果以截图形式存在，可启用 OCR（需本机安装 Tesseract）：

```bash
# macOS (Homebrew)
brew install tesseract

# Windows (winget / 官方安装包)
winget install UB-Mannheim.TesseractOCR
```

未安装 Tesseract 时，文件解析仍可用；仅 OCR 截图识别不可用。可用 `imgforge doctor` 检查依赖。

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
- **图片评审**（`review` feature，GUI 默认开启）
- **视频评审**（`video-review` feature，GUI 默认开启；依赖外部 ffmpeg/ffprobe）
- **数据提取**（`data-extract` feature，GUI 默认开启；OCR 依赖外部 tesseract）
- **Bayer/RAW**（`bayer` feature，GUI 默认开启）
- **libvips 后端**：占位实现，当前回退原生后端（`doctor` 会标注可用性）

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

预编译包已内置常用扩展：增量处理、重命名、缩略图、水印、JPEG XL。

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
git tag v0.1.7
git push origin v0.1.7
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
cargo build --release --features "incremental,rename,thumbnails,watermark,jpegxl,vips"

# GUI（捆绑 review / video-review / data-extract / jpegxl / bayer 等）
cargo build --release --features gui --bin imgforge-app

# Bayer/RAW 马赛克解码
cargo build --release --features bayer

# libvips 后端（需系统安装 libvips，如 `brew install vips`）
cargo build --release --features vips
```

> **注意**：`--features vips` 需要系统已安装 libvips 开发库；运行时若初始化失败会自动回退原生后端。
>
> **Feature 差异**：本地 `cargo build` 默认不含 P2/GUI；预编译 CLI 包通常启用 `incremental,rename,thumbnails,watermark,jpegxl,bayer`；GUI 包启用 `gui`。用 `imgforge doctor` 查看当前二进制实际启用的 feature 与运行时依赖。

### 测试与基准

```bash
# 默认 feature 单元/集成测试
cargo test

# 含增量等常见 P2 feature
cargo test --features "incremental,rename,thumbnails,watermark"

# GUI 相关模块测试（需本机 GUI 依赖）
cargo test --features gui

# 转换/缩放/扫描性能基准
cargo bench --bench conversion_bench
```

大图策略：执行器会按最大输入体积动态收紧并发（约 ≥32 MiB 限 2，≥128 MiB 限 1），降低全量读入内存时的峰值占用。
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

检查运行时平台、已启用 feature、后端状态，以及远端接入配置摘要。

### 远端服务器接入

远端模式以服务器栈为主：客户端上传素材，`imgforge-server` 通过 Postgres / Redis Streams / S3-MinIO 管理任务、队列与产物，Worker 完成转换、评审、视频评审和数据提取。SQLite / 磁盘 / 内存队列仅作为测试和单机开发后备，不作为产品架构推荐。

```powershell
# 1) 启动远端栈（Postgres + Redis + MinIO + imgforge-server）
docker compose -f deploy/docker-compose.yml up --build

# 2) 客户端配置（另一终端 / 另一台机器）
$env:IMGFORGE_REMOTE_ENABLED = "true"
$env:IMGFORGE_REMOTE_BASE_URL = "http://127.0.0.1:8787"
$env:IMGFORGE_REMOTE_AUTH_MODE = "env_bearer"
$env:IMGFORGE_REMOTE_TOKEN = "change-me"

# 查看健康状态
imgforge remote status

# 上传、远端转换并下载结果
imgforge remote submit -i .\photos -o .\output -f webp

# 或在普通转换命令上加 --remote（同样会等待完成并下载）
imgforge -i .\photos -o .\output -f webp --remote
```

相关环境变量：

| 变量 | 说明 |
|------|------|
| `IMGFORGE_REMOTE_ENABLED` | 启用远端（`true`/`1`） |
| `IMGFORGE_REMOTE_BASE_URL` | API 根地址（如 `http://127.0.0.1:8787`） |
| `IMGFORGE_REMOTE_WORKSPACE_ID` | 工作区 ID |
| `IMGFORGE_REMOTE_AUTH_MODE` | `none` / `env_bearer` / `keychain` |
| `IMGFORGE_REMOTE_TOKEN` | Bearer token（勿写入 TOML） |
| `IMGFORGE_REMOTE_TIMEOUT_SECS` | 超时秒数 |
| `IMGFORGE_SERVER_BIND` | 服务端监听地址（默认 `127.0.0.1:8787`） |
| `IMGFORGE_SERVER_TOKEN` | 服务端可选 Bearer |
| `IMGFORGE_PUBLIC_BASE` | 对外 API base（默认 `http://127.0.0.1:8787`） |
| `IMGFORGE_SERVER_DATA_DIR` | 服务器数据目录（默认 `~/.imgforge/server`） |
| `IMGFORGE_INLINE_WORKER` | 是否在 API 进程内跑 Worker（默认开） |
| `IMGFORGE_RATE_LIMIT_PER_MINUTE` | 每 token/IP 轻量限流（默认 `120`） |
| `IMGFORGE_DATABASE_URL` / `DATABASE_URL` | Postgres 元数据 |
| `IMGFORGE_REDIS_URL` / `REDIS_URL` | Redis Streams 队列 |
| `IMGFORGE_S3_ENDPOINT` / `IMGFORGE_S3_BUCKET` | S3/MinIO 对象存储 |

GUI：转换页勾选「优先提交远端任务」后，开始转换会上传文件、等待远端完成并下载到输出目录；任务中心可同步/刷新远端任务，并可从完成的评审 / 视频评审 / 数据提取任务跳转到对应模块。配置 `remote.enabled` + `remote.base_url` 后，模块侧边栏会提供「远程 / 本地」数据源切换；远端目录不可达时回退本地，下载的缩略图与数据提取报告缓存到 `~/.imgforge/remote_cache/assets`。

约定 API（schema v1）：

**控制面**

- `GET /v1/health`
- `POST /v1/jobs`（body: `RemoteJobRequest`，支持 `client_request_id` 幂等）
- `GET /v1/jobs?limit=`
- `GET /v1/jobs/{id}`
- `GET /v1/jobs/{id}/result`
- `POST /v1/jobs/{id}/cancel`
- `GET /v1/jobs/{id}/events`（SSE）
- `GET /v1/jobs/{id}/events/poll?after=`（轮询兼容）

统一错误体：`{ code, message, retryable, details?, request_id? }`。客户端对 429/5xx / `retryable` 错误做有限次退避重试。

**数据面**

- `POST /v1/uploads:init` → `RemoteUploadSession`（含 `PUT` URL）
- `PUT /v1/uploads/{id}/bytes` → 上传文件内容
- `POST /v1/uploads:complete` → `RemoteAssetRef`
- `POST /v1/uploads:abort`
- `GET /v1/artifacts/{id}/download` → 短期下载凭证
- `GET /v1/artifacts/{id}/content` → 直接下载文件字节

**数据加载**

- `GET /v1/assets`
- `GET /v1/review/batches`
- `GET /v1/extract/results`

服务端（`src/server/`，feature `server`）：axum 路由、Postgres `JobStore`、Redis Streams 队列、S3/MinIO `ObjectStore`、内联 Worker（复用 `run_batch`）。未配置远端依赖时会回退 SQLite / 磁盘 / 内存队列，仅用于测试和开发。部署细节见 `docs/remote-deploy.md`。

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
