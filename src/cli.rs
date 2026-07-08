//! clap 命令行参数定义。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};

use imgforge::core::types::ImageFormat;

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
