//! 子进程启动辅助：Windows GUI 下隐藏控制台闪窗。

use std::process::Command;

/// 为即将执行的外部命令配置平台相关标志。
///
/// Windows 上为 GUI 子系统宿主进程附加 `CREATE_NO_WINDOW`，避免
/// ffmpeg / ffprobe / adb 等控制台程序每次弹出黑窗。
pub fn configure_command(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// 创建已配置好的 `Command`（等价于 `Command::new` + [`configure_command`]）。
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    configure_command(&mut cmd);
    cmd
}
