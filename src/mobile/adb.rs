//! ADB 拉取后端。

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::core::error::{AppError, AppResult};
use crate::mobile::adb_binary::resolve_adb_binary;
use crate::mobile::{
    ensure_cancelled_not_set, is_supported_media_remote, safe_remote_relative, MobilePullConfig,
    MobilePullOutcome,
};
use crate::ui::progress::ProgressReporter;

pub fn pull(
    config: &MobilePullConfig,
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> AppResult<MobilePullOutcome> {
    std::fs::create_dir_all(&config.staging_dir)
        .map_err(|e| AppError::io(&config.staging_dir, e))?;

    let adb = resolve_adb_binary(config)?;
    let runner = ProcessAdbRunner { adb };
    runner.check_version()?;
    let serial = select_device(&runner, config.adb_serial.as_deref())?;
    let remote_files = list_remote_media(&runner, &serial, &config.source_path)?;

    if let Some(progress) = &progress {
        progress.set_total(remote_files.len());
        progress.set_current_label("正在通过 ADB 拉取文件");
    }

    let mut local_files = Vec::with_capacity(remote_files.len());
    for remote in remote_files {
        ensure_cancelled_not_set(&cancelled)?;
        let relative = if config.preserve_structure {
            safe_remote_relative(&config.source_path, &remote)?
        } else {
            PathBuf::from(
                remote
                    .rsplit('/')
                    .next()
                    .filter(|name| !name.is_empty())
                    .unwrap_or("media"),
            )
        };
        let local = config.staging_dir.join(relative);
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        if !local.exists() {
            runner.pull(&serial, &remote, &local)?;
        }
        local_files.push(local);
        if let Some(progress) = &progress {
            progress.set_current_label(remote.rsplit('/').next().unwrap_or("media"));
            progress.inc(None);
        }
    }

    Ok(MobilePullOutcome {
        staging_dir: config.staging_dir.clone(),
        files: local_files,
    })
}

trait AdbRunner {
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
                &format!("adb pull {remote} {}", local.display()),
                &output,
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdbDevice {
    serial: String,
    state: String,
}

fn select_device(runner: &impl AdbRunner, requested: Option<&str>) -> AppResult<String> {
    let output = runner.run(["devices", "-l"])?;
    if !output.status.success() {
        return Err(adb_command_error("adb devices -l", &output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let devices = parse_devices(&stdout);

    if let Some(serial) = requested {
        let Some(device) = devices.iter().find(|d| d.serial == serial) else {
            return Err(AppError::Config(format!(
                "adb device '{serial}' was not found"
            )));
        };
        ensure_device_ready(device)?;
        return Ok(serial.to_string());
    }

    let ready: Vec<_> = devices.iter().filter(|d| d.state == "device").collect();
    if ready.len() == 1 {
        return Ok(ready[0].serial.clone());
    }
    if ready.len() > 1 {
        return Err(AppError::Config(
            "multiple adb devices are connected; set --adb-serial".into(),
        ));
    }

    if let Some(device) = devices.first() {
        ensure_device_ready(device)?;
    }

    Err(AppError::Config(
        "no authorized adb device found; connect an Android device and enable USB debugging".into(),
    ))
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

fn parse_devices(text: &str) -> Vec<AdbDevice> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("List of devices") {
                return None;
            }
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let state = parts.next()?.to_string();
            Some(AdbDevice { serial, state })
        })
        .collect()
}

fn ensure_device_ready(device: &AdbDevice) -> AppResult<()> {
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

    #[test]
    fn parse_adb_devices_output() {
        let devices = parse_devices(
            "List of devices attached\nemulator-5554\tdevice product:sdk\nabc123 unauthorized\n",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].state, "device");
        assert_eq!(devices[1].state, "unauthorized");
    }

    #[test]
    fn unauthorized_device_is_actionable() {
        let err = ensure_device_ready(&AdbDevice {
            serial: "abc".into(),
            state: "unauthorized".into(),
        })
        .unwrap_err();
        assert!(err.to_string().contains("unauthorized"));
    }
}
