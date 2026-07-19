//! ADB 拉取后端（支持多设备并行 + 台内文件并发）。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::core::error::{AppError, AppResult};
use crate::mobile::adb_binary::resolve_adb_binary;
use crate::mobile::{
    ensure_cancelled_not_set, is_supported_media_remote, run_parallel_jobs, safe_remote_relative,
    sanitize_serial, MobilePullConfig, MobilePullOutcome, ResolvedDeviceTarget,
};
use crate::ui::progress::ProgressReporter;

/// 已连接 ADB 设备信息（供 GUI 列表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDeviceInfo {
    pub serial: String,
    pub state: String,
    pub model: Option<String>,
}

impl AdbDeviceInfo {
    pub fn is_ready(&self) -> bool {
        self.state == "device"
    }

    pub fn display_label(&self) -> String {
        match &self.model {
            Some(m) if !m.is_empty() => format!("{} ({m})", self.serial),
            _ => self.serial.clone(),
        }
    }
}

/// 列出当前 ADB 设备（含未就绪），用于 GUI 刷新。
pub fn list_devices(config: &MobilePullConfig) -> AppResult<Vec<AdbDeviceInfo>> {
    let adb = resolve_adb_binary(config)?;
    let runner = ProcessAdbRunner { adb };
    runner.check_version()?;
    list_all_devices(&runner)
}

/// 仅返回已授权就绪设备。
pub fn list_ready_devices(config: &MobilePullConfig) -> AppResult<Vec<AdbDeviceInfo>> {
    Ok(list_devices(config)?
        .into_iter()
        .filter(|d| d.is_ready())
        .collect())
}

pub fn pull(
    config: &MobilePullConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<MobilePullOutcome> {
    let adb = resolve_adb_binary(config)?;
    let runner = Arc::new(ProcessAdbRunner { adb });
    runner.check_version()?;

    let all = list_all_devices(runner.as_ref())?;
    let requested = config.effective_device_targets();
    let serials_only: Vec<String> = requested.iter().map(|t| t.serial.clone()).collect();
    let resolved_serials = resolve_target_serials(&all, &serials_only)?;

    // 预扫描各机文件数以设总进度；每台可用独立 source / staging
    let mut device_plans = Vec::with_capacity(resolved_serials.len());
    let mut total_files = 0usize;
    for serial in &resolved_serials {
        ensure_cancelled_not_set(&cancelled)?;
        let target = requested
            .iter()
            .find(|t| t.serial == *serial)
            .cloned()
            .unwrap_or_else(|| ResolvedDeviceTarget {
                serial: serial.clone(),
                source_path: config.source_path.trim().to_string(),
                staging_root: config.staging_dir.join(sanitize_serial(serial)),
            });
        let remote_files = list_remote_media(runner.as_ref(), serial, &target.source_path)?;
        total_files += remote_files.len();
        device_plans.push((target, remote_files));
    }

    if let Some(progress) = &progress {
        progress.set_total(total_files);
        let label = format!("正在从 {} 台设备并行拉取", device_plans.len());
        progress.set_current_label(&label);
    }

    // 确保各设备保存根目录存在（共用暂存根也一并创建）
    if !config.staging_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&config.staging_dir)
            .map_err(|e| AppError::io(&config.staging_dir, e))?;
    }
    for (target, _) in &device_plans {
        std::fs::create_dir_all(&target.staging_root)
            .map_err(|e| AppError::io(&target.staging_root, e))?;
    }

    let file_concurrency = config.effective_concurrency();
    let preserve = config.preserve_structure;
    let progress = progress.clone();
    let cancelled_ref = Arc::clone(&cancelled);
    let device_concurrency = device_plans.len().max(1);

    // 设备全开并行；台内再用 file_concurrency
    let device_results = run_parallel_jobs(device_plans, device_concurrency, &cancelled, |plan| {
        let (target, remote_files) = plan;
        pull_one_device(
            runner.as_ref(),
            &target.serial,
            remote_files,
            &target.staging_root,
            &target.source_path,
            preserve,
            file_concurrency,
            &cancelled_ref,
            progress.as_ref(),
        )
    })?;

    let mut files = Vec::new();
    for mut batch in device_results {
        files.append(&mut batch);
    }

    Ok(MobilePullOutcome {
        staging_dir: config.staging_dir.clone(),
        files,
    })
}

fn pull_one_device(
    runner: &ProcessAdbRunner,
    serial: &str,
    remote_files: &[String],
    device_root: &Path,
    source_path: &str,
    preserve_structure: bool,
    file_concurrency: usize,
    cancelled: &AtomicBool,
    progress: Option<&Arc<dyn ProgressReporter>>,
) -> AppResult<Vec<PathBuf>> {
    std::fs::create_dir_all(device_root).map_err(|e| AppError::io(device_root, e))?;

    struct Job {
        remote: String,
        local: PathBuf,
        need_pull: bool,
    }

    let mut jobs = Vec::with_capacity(remote_files.len());
    for remote in remote_files {
        ensure_cancelled_not_set(cancelled)?;
        let relative = if preserve_structure {
            safe_remote_relative(source_path, remote)?
        } else {
            PathBuf::from(
                remote
                    .rsplit('/')
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("media"),
            )
        };
        let local = device_root.join(relative);
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        let need_pull = !local.exists();
        jobs.push(Job {
            remote: remote.clone(),
            local,
            need_pull,
        });
    }

    let serial_owned = serial.to_string();
    run_parallel_jobs(jobs, file_concurrency, cancelled, |job| {
        if job.need_pull {
            runner.pull(&serial_owned, &job.remote, &job.local)?;
        }
        if let Some(progress) = progress {
            let name = job.remote.rsplit('/').next().unwrap_or("media");
            let label = format!("{serial_owned}: {name}");
            progress.set_current_label(&label);
            progress.inc(None);
        }
        Ok(job.local.clone())
    })
}

/// 根据配置与已连接设备解析目标 serial 列表。
pub fn resolve_target_serials(
    all: &[AdbDeviceInfo],
    requested: &[String],
) -> AppResult<Vec<String>> {
    if requested.is_empty() {
        let ready: Vec<String> = all
            .iter()
            .filter(|d| d.is_ready())
            .map(|d| d.serial.clone())
            .collect();
        if ready.is_empty() {
            if let Some(device) = all.first() {
                ensure_device_ready_info(device)?;
            }
            return Err(AppError::Config(
                "no authorized adb device found; connect an Android device and enable USB debugging"
                    .into(),
            ));
        }
        return Ok(ready);
    }

    let mut out = Vec::with_capacity(requested.len());
    for serial in requested {
        let Some(device) = all.iter().find(|d| d.serial == *serial) else {
            return Err(AppError::Config(format!(
                "adb device '{serial}' was not found"
            )));
        };
        ensure_device_ready_info(device)?;
        out.push(serial.clone());
    }
    Ok(out)
}

fn list_all_devices(runner: &impl AdbRunner) -> AppResult<Vec<AdbDeviceInfo>> {
    let output = runner.run(["devices", "-l"])?;
    if !output.status.success() {
        return Err(adb_command_error("adb devices -l", &output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_devices(&stdout))
}

trait AdbRunner: Sync {
    fn run<I, S>(&self, args: I) -> AppResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>;
}

struct ProcessAdbRunner {
    adb: PathBuf,
}

impl AdbRunner for ProcessAdbRunner {
    fn run<I, S>(&self, args: I) -> AppResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        crate::process_util::command(&self.adb)
            .args(args)
            .output()
            .map_err(|e| AppError::io(&self.adb, e))
    }
}

impl ProcessAdbRunner {
    fn check_version(&self) -> AppResult<()> {
        let output = self.run(["version"])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(adb_command_error("adb version", &output))
        }
    }

    fn pull(&self, serial: &str, remote: &str, local: &PathBuf) -> AppResult<()> {
        let args = vec![
            OsString::from("-s"),
            OsString::from(serial),
            OsString::from("pull"),
            OsString::from(remote),
            local.as_os_str().to_os_string(),
        ];
        let output = self.run(args)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(adb_command_error(
                &format!("adb -s {serial} pull {remote} {}", local.display()),
                &output,
            ))
        }
    }
}

fn list_remote_media(
    runner: &impl AdbRunner,
    serial: &str,
    source_path: &str,
) -> AppResult<Vec<String>> {
    let output = runner.run([
        OsString::from("-s"),
        OsString::from(serial),
        OsString::from("shell"),
        OsString::from("find"),
        OsString::from(source_path),
        OsString::from("-type"),
        OsString::from("f"),
    ])?;
    if !output.status.success() {
        return Err(adb_command_error(
            &format!("adb shell find {source_path} -type f"),
            &output,
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with("find:") {
            continue;
        }
        if is_supported_media_remote(line) {
            files.push(line.to_string());
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn parse_devices(text: &str) -> Vec<AdbDeviceInfo> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("List of devices") {
                return None;
            }
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            let mut model = None;
            for part in parts {
                if let Some(m) = part.strip_prefix("model:") {
                    model = Some(m.replace('_', " "));
                }
            }
            Some(AdbDeviceInfo {
                serial,
                state,
                model,
            })
        })
        .collect()
}

fn ensure_device_ready_info(device: &AdbDeviceInfo) -> AppResult<()> {
    match device.state.as_str() {
        "device" => Ok(()),
        "unauthorized" => Err(AppError::Config(format!(
            "adb device '{}' is unauthorized; unlock the phone and accept the USB debugging RSA prompt",
            device.serial
        ))),
        "offline" => Err(AppError::Config(format!(
            "adb device '{}' is offline; reconnect the device or restart adb server",
            device.serial
        ))),
        other => Err(AppError::Config(format!(
            "adb device '{}' is not ready: {other}",
            device.serial
        ))),
    }
}

fn adb_command_error(command: &str, output: &Output) -> AppError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("exit status {}", output.status)
    };
    AppError::Config(format!("{command} failed: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn parse_adb_devices_output() {
        let devices = parse_devices(
            "List of devices attached\nemulator-5554\tdevice product:sdk_gphone model:sdk_gphone64_arm64\nabc123 unauthorized\n",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].state, "device");
        assert_eq!(devices[0].model.as_deref(), Some("sdk gphone64 arm64"));
        assert_eq!(devices[1].state, "unauthorized");
    }

    #[test]
    fn unauthorized_device_is_actionable() {
        let err = ensure_device_ready_info(&AdbDeviceInfo {
            serial: "abc".into(),
            state: "unauthorized".into(),
            model: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
    }

    #[test]
    fn resolve_all_ready_when_unspecified() {
        let all = vec![
            AdbDeviceInfo {
                serial: "a".into(),
                state: "device".into(),
                model: None,
            },
            AdbDeviceInfo {
                serial: "b".into(),
                state: "device".into(),
                model: None,
            },
            AdbDeviceInfo {
                serial: "c".into(),
                state: "unauthorized".into(),
                model: None,
            },
        ];
        assert_eq!(resolve_target_serials(&all, &[]).unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn resolve_requested_filters() {
        let all = vec![AdbDeviceInfo {
            serial: "a".into(),
            state: "device".into(),
            model: None,
        }];
        assert_eq!(
            resolve_target_serials(&all, &["a".into()]).unwrap(),
            vec!["a"]
        );
        assert!(resolve_target_serials(&all, &["missing".into()]).is_err());
    }

    #[test]
    fn sanitize_serial_strips_unsafe() {
        assert_eq!(sanitize_serial("ab:cd/ef"), "ab_cd_ef");
        assert_eq!(sanitize_serial(""), "device");
    }

    #[test]
    fn parallel_jobs_respects_concurrency_peak() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let cancelled = AtomicBool::new(false);
        let jobs: Vec<usize> = (0..8).collect();
        let _ = run_parallel_jobs(jobs, 2, &cancelled, |_| {
            let now = active.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            peak.fetch_max(now, AtomicOrdering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            active.fetch_sub(1, AtomicOrdering::SeqCst);
            Ok(())
        })
        .unwrap();
        assert!(
            peak.load(AtomicOrdering::SeqCst) <= 2,
            "peak concurrency was {}",
            peak.load(AtomicOrdering::SeqCst)
        );
        assert!(peak.load(AtomicOrdering::SeqCst) >= 1);
    }
}
