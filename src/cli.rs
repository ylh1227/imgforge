//! clap 命令行参数定义。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};

use imgforge::core::types::ImageFormat;
use imgforge::mobile::{AdbBinaryMode, MobilePullBackend};

/// imgforge — 跨平台高性能批量图片格式转换工具
#[derive(Parser, Debug)]
#[command(
  name = "imgforge",
  version,
  about = "Cross-platform batch image format converter",
  long_about = None
)]
pub struct Cli {
    /// 输入目录
    #[arg(short, long, value_name = "DIR", env = "IMGFORGE_INPUT")]
    pub input: Option<PathBuf>,

    /// 输出目录
    #[arg(short, long, value_name = "DIR", env = "IMGFORGE_OUTPUT")]
    pub output: Option<PathBuf>,

    /// 目标格式
    #[arg(short = 'f', long, value_name = "FORMAT", env = "IMGFORGE_FORMAT")]
    pub format: Option<ImageFormat>,

    /// 输出质量 (1-100)
    #[arg(short, long, value_name = "N", env = "IMGFORGE_QUALITY")]
    pub quality: Option<u8>,

    /// 并发数
    #[arg(short = 'j', long, value_name = "N", env = "IMGFORGE_CONCURRENCY")]
    pub concurrency: Option<usize>,

    /// 递归扫描子目录
    #[arg(long, default_value_t = true)]
    pub recursive: bool,

    /// 不递归扫描
    #[arg(long)]
    pub no_recursive: bool,

    /// 覆盖已存在的输出文件
    #[arg(long)]
    pub overwrite: bool,

    /// 保留输入目录结构
    #[arg(long, default_value_t = true)]
    pub preserve_structure: bool,

    /// 扁平输出（不保留目录结构）
    #[arg(long)]
    pub flat: bool,

    /// 预览模式，不实际写入
    #[arg(long)]
    pub dry_run: bool,

    /// 目标宽度
    #[arg(long)]
    pub width: Option<u32>,

    /// 目标高度
    #[arg(long)]
    pub height: Option<u32>,

    /// 亮度调整 (-1.0 ~ 1.0)
    #[arg(long)]
    pub brightness: Option<f32>,

    /// 对比度调整 (-1.0 ~ 1.0)
    #[arg(long)]
    pub contrast: Option<f32>,

    /// 锐化强度 (0.0 ~ 2.0)
    #[arg(long)]
    pub sharpen: Option<f32>,

    /// 参考图亮度匹配：参考 JPG/PNG 路径（全局模式）
    #[arg(long, value_name = "PATH")]
    pub brightness_ref: Option<PathBuf>,

    /// 按文件配对亮度匹配（同目录同名 jpg/jpeg/png/webp）
    #[arg(long)]
    pub brightness_pair: bool,

    /// 参考亮度统计：mean 或 percentile
    #[arg(long, value_enum)]
    pub brightness_metric: Option<imgforge::core::types::BrightnessMatchMetric>,

    /// 百分位 (0-100)，仅 percentile 模式
    #[arg(long, default_value_t = 98.0)]
    pub brightness_percentile: f32,

    /// 启用分区亮度匹配（默认 3×3）
    #[arg(long)]
    pub brightness_regional: bool,

    /// 剥离 EXIF 元数据
    #[arg(long)]
    pub strip_metadata: bool,

    /// 保留 EXIF 元数据
    #[arg(long)]
    pub keep_metadata: bool,

    /// 仅处理指定后缀（逗号分隔）
    #[arg(long, value_delimiter = ',')]
    pub extensions: Option<Vec<String>>,

    /// 最小文件大小（字节）
    #[arg(long)]
    pub min_size: Option<u64>,

    /// 最大文件大小（字节）
    #[arg(long)]
    pub max_size: Option<u64>,

    /// 使用内置预设 (web, minimal, print)
    #[arg(long, value_name = "NAME")]
    pub preset: Option<String>,

    /// TOML 配置文件路径
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// 详细日志
    #[arg(short, long)]
    pub verbose: bool,

    /// 增量处理：仅转换新增/修改的文件（需 incremental feature）
    #[arg(long)]
    pub incremental: bool,

    /// 输出文件名模板（需 rename feature），占位符：{stem} {name} {ext} {dir} {index}
    #[arg(long, value_name = "TEMPLATE")]
    pub rename_template: Option<String>,

    /// 图片水印路径（需 watermark feature）
    #[arg(long, value_name = "FILE")]
    pub watermark_image: Option<PathBuf>,

    /// 文字水印内容（需 watermark feature）
    #[arg(long, value_name = "TEXT")]
    pub watermark_text: Option<String>,

    /// 文字水印字体路径（需 watermark feature）
    #[arg(long, value_name = "FILE")]
    pub watermark_font: Option<PathBuf>,

    /// 水印不透明度 (0.0-1.0)
    #[arg(long, default_value_t = 0.5)]
    pub watermark_opacity: f32,

    /// 水印字号
    #[arg(long, default_value_t = 24.0)]
    pub watermark_size: f32,

    /// 多尺寸缩略图（需 thumbnails feature），如：256,512,1024x768
    #[arg(long, value_delimiter = ',', value_name = "SIZE")]
    pub thumbnail_sizes: Option<Vec<String>>,

    /// 旋转变换 (rotate90, rotate180, flip_horizontal 等)
    #[arg(long, value_enum)]
    pub transform: Option<imgforge::core::types::Transform>,

    /// 缩放模式 (fit, fill, exact)
    #[arg(long, value_enum)]
    pub resize_mode: Option<imgforge::core::types::ResizeMode>,

    /// 仅解 Bayer/RAW 马赛克（跳过缩放/锐化/水印；需 --features bayer）
    #[arg(long, env = "IMGFORGE_BAYER_ONLY")]
    pub bayer_only: bool,

    /// 水印位置
    #[arg(long, value_enum)]
    pub watermark_position: Option<imgforge::core::types::WatermarkPosition>,

    /// 是否优先远端执行（默认 false）
    #[arg(long, env = "IMGFORGE_REMOTE")]
    pub remote: bool,

    /// 转换前从移动设备拉取媒体到本地暂存目录
    #[arg(long, env = "IMGFORGE_MOBILE_PULL")]
    pub mobile_pull: bool,

    /// 移动设备拉取后端
    #[arg(long, value_enum, env = "IMGFORGE_MOBILE_BACKEND")]
    pub mobile_backend: Option<MobilePullBackend>,

    /// 移动设备来源路径；ADB 默认为 /sdcard/DCIM，本地挂载模式为本地目录
    #[arg(long, value_name = "PATH", env = "IMGFORGE_MOBILE_SOURCE")]
    pub mobile_source: Option<String>,

    /// 移动设备拉取暂存目录
    #[arg(long, value_name = "DIR", env = "IMGFORGE_MOBILE_STAGING")]
    pub mobile_staging: Option<PathBuf>,

    /// ADB 设备 serial；可重复或逗号分隔指定多台（留空则拉全部已授权设备）
    #[arg(
        long,
        value_name = "SERIAL",
        env = "IMGFORGE_ADB_SERIAL",
        num_args = 0..,
        value_delimiter = ','
    )]
    pub adb_serial: Vec<String>,

    /// 移动设备拉取并发数（1–8，默认 4）
    #[arg(long, value_name = "N", env = "IMGFORGE_MOBILE_CONCURRENCY")]
    pub mobile_concurrency: Option<usize>,

    /// ADB 二进制选择策略
    #[arg(long, value_enum, env = "IMGFORGE_ADB_MODE")]
    pub adb_mode: Option<AdbBinaryMode>,

    /// 自定义 ADB 路径；未指定时优先使用程序内置 ADB
    #[arg(long, value_name = "FILE", env = "IMGFORGE_ADB_PATH")]
    pub adb_path: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 生成 shell 自动补全脚本
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    /// 检查环境与 feature 状态
    Doctor,
    /// 远端服务器接入（预留）：状态检查与任务同步
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum RemoteCommands {
    /// 显示远端配置与健康状态
    Status,
    /// 拉取远端任务列表（未配置时回退本地缓存）
    Pull {
        /// 最多返回条数
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// 提交远端任务；默认按当前转换参数提交 convert
    Submit {
        /// 任务来源
        #[arg(long, value_enum, default_value_t = RemoteSubmitSource::Convert)]
        source: RemoteSubmitSource,
        /// review/video/extract 输入路径；为空时使用 -i/--input
        #[arg(value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum, Debug, PartialEq, Eq)]
pub enum RemoteSubmitSource {
    Convert,
    Review,
    Video,
    Extract,
}

#[derive(Clone, ValueEnum, Debug)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Elvish => Shell::Elvish,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::PowerShell => Shell::PowerShell,
            CompletionShell::Zsh => Shell::Zsh,
        }
    }
}

impl Cli {
    pub fn generate_completions(shell: Shell) {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        generate(shell, &mut cmd, "imgforge", &mut std::io::stdout());
    }
}
