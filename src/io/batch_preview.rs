//! 转换前扫描摘要（dry-run / GUI 预估）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::AppConfig;
use crate::core::error::AppResult;
use crate::io::scanner::{tasks_from_paths, ScanFilter, ScanOptions};
use crate::scheduler::task::ConversionTask;

/// 单条输出样例。
#[derive(Debug, Clone)]
pub struct PreviewSample {
    pub input: PathBuf,
    pub output: PathBuf,
}

/// 批量转换前摘要。
#[derive(Debug, Clone, Default)]
pub struct BatchPreview {
    /// 扫描到的可转换文件总数（含将被跳过的）。
    pub matched: usize,
    /// 实际将转换的文件数。
    pub to_convert: usize,
    /// 因输出已存在且未勾选覆盖而跳过。
    pub skipped_existing: usize,
    /// 重命名模板导致的输出路径冲突数。
    pub output_conflicts: usize,
    /// 前 N 条输入→输出样例。
    pub samples: Vec<PreviewSample>,
    /// 冲突输出路径示例。
    pub conflict_examples: Vec<String>,
}

impl BatchPreview {
    pub fn summary_lines(&self, format: &str) -> Vec<String> {
        let mut lines = vec![
            format!("匹配图片：{} 张", self.matched),
            format!("将转换：{} 张 → {}", self.to_convert, format),
        ];
        if self.skipped_existing > 0 {
            lines.push(format!("跳过（输出已存在）：{} 张", self.skipped_existing));
        }
        if self.output_conflicts > 0 {
            lines.push(format!(
                "输出路径冲突：{} 处（请调整重命名模板）",
                self.output_conflicts
            ));
        }
        lines
    }
}

/// 根据当前配置生成转换前摘要（不读写磁盘、不编码）。
pub fn preview_batch(config: &AppConfig) -> AppResult<BatchPreview> {
    let tasks = collect_tasks(config)?;
    analyze_tasks(&tasks, config.overwrite)
}

fn collect_tasks(config: &AppConfig) -> AppResult<Vec<ConversionTask>> {
    if !config.explicit_inputs.is_empty() {
        return tasks_from_paths(
            &config.explicit_inputs,
            &config.output_dir,
            config.target_format,
            true, // 预览阶段不跳过已存在，由 analyze 统计
            config.bayer_only,
        );
    }

    let options = ScanOptions {
        input_dir: config.input_dir.clone(),
        output_dir: config.output_dir.clone(),
        target_format: config.target_format,
        recursive: config.recursive,
        preserve_structure: config.preserve_structure,
        overwrite: true,
        filter: ScanFilter {
            extensions: config.extensions.clone(),
            min_size: config.min_size,
            max_size: config.max_size,
            modified_after: None,
            modified_before: None,
        },
        rename_template: config.rename_template.clone(),
        bayer_only: config.bayer_only,
    };

    crate::io::scanner::scan_inputs(&options)
}

fn analyze_tasks(tasks: &[ConversionTask], overwrite: bool) -> AppResult<BatchPreview> {
    let matched = tasks.len();
    let mut output_map: HashMap<PathBuf, usize> = HashMap::new();
    let mut conflict_examples = Vec::new();

    for task in tasks {
        let canon = crate::io::paths::canonicalize(&task.output_path);
        let count = output_map.entry(canon.clone()).or_insert(0);
        *count += 1;
        if *count == 2 && conflict_examples.len() < 5 {
            conflict_examples.push(canon.display().to_string());
        }
    }

    let output_conflicts = output_map.values().filter(|&&c| c > 1).count();

    let mut skipped_existing = 0usize;
    let mut to_convert = 0usize;
    let mut samples = Vec::new();

    for task in tasks {
        let exists = task.output_path.exists();
        if exists && !overwrite {
            skipped_existing += 1;
            continue;
        }
        to_convert += 1;
        if samples.len() < 5 {
            samples.push(PreviewSample {
                input: task.input_path.clone(),
                output: task.output_path.clone(),
            });
        }
    }

    Ok(BatchPreview {
        matched,
        to_convert,
        skipped_existing,
        output_conflicts,
        samples,
        conflict_examples,
    })
}

/// 为重命名模板预览：从输入目录取前 `limit` 个文件并解析输出名。
#[cfg(feature = "rename")]
pub fn rename_preview_samples(
    input_dir: &Path,
    output_dir: &Path,
    template: &str,
    target_format: crate::core::types::ImageFormat,
    preserve_structure: bool,
    recursive: bool,
    limit: usize,
) -> AppResult<Vec<(PathBuf, Result<String, String>)>> {
    use crate::io::scanner::{ScanFilter, ScanOptions};

    if template.trim().is_empty() {
        return Ok(Vec::new());
    }

    let options = ScanOptions {
        input_dir: input_dir.to_path_buf(),
        output_dir: output_dir.to_path_buf(),
        target_format,
        recursive,
        preserve_structure,
        overwrite: true,
        filter: ScanFilter::default(),
        rename_template: Some(template.to_string()),
        bayer_only: false,
    };

    let tasks = crate::io::scanner::scan_inputs(&options)?;
    Ok(tasks
        .iter()
        .take(limit)
        .map(|t| {
            let name = t
                .output_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (t.input_path.clone(), Ok(name))
        })
        .collect())
}

#[cfg(not(feature = "rename"))]
pub fn rename_preview_samples(
    _input_dir: &Path,
    _output_dir: &Path,
    template: &str,
    _target_format: crate::core::types::ImageFormat,
    _preserve_structure: bool,
    _recursive: bool,
    _limit: usize,
) -> AppResult<Vec<(PathBuf, Result<String, String>)>> {
    if template.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::task::ConversionTask;

    #[test]
    fn analyze_counts_skip_and_conflicts() {
        let tasks = vec![
            ConversionTask::new("/in/a.jpg".into(), "/out/a.webp".into(), 100),
            ConversionTask::new("/in/b.jpg".into(), "/out/a.webp".into(), 100),
            ConversionTask::new("/in/c.jpg".into(), "/out/c.webp".into(), 100),
        ];
        let preview = analyze_tasks(&tasks, false).unwrap();
        assert_eq!(preview.matched, 3);
        assert_eq!(preview.output_conflicts, 1);
        assert_eq!(preview.to_convert, 3);
        assert_eq!(preview.skipped_existing, 0);
    }
}
