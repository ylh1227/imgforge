//! ImgForge 图形界面入口（双击运行，无需命令行）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

fn main() -> eframe::Result<()> {
    install_crash_hooks();

    let options = eframe::NativeOptions {
        viewport: app_viewport(),
        centered: true,
        ..Default::default()
    };

    let result = eframe::run_native(
        "ImgForge",
        options,
        Box::new(|cc| Ok(Box::new(imgforge::gui::ImgforgeApp::new(cc)))),
    );

    if let Err(ref err) = result {
        report_fatal(&format!("窗口初始化失败：{err}"));
    }
    result
}

fn app_viewport() -> egui::ViewportBuilder {
    let builder = egui::ViewportBuilder::default()
        .with_inner_size([840.0, 740.0])
        .with_min_inner_size([700.0, 580.0])
        .with_title("ImgForge")
        .with_app_id("com.imgforge.app");

    #[cfg(target_os = "macos")]
    {
        // 全尺寸内容视图 + 系统标题栏；底部操作栏由 AppKit NSGlassEffectView 原生层渲染
        return builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(true)
            .with_titlebar_buttons_shown(true);
    }

    #[cfg(not(target_os = "macos"))]
    {
        builder
    }
}

fn install_crash_hooks() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = panic_payload_str(info.payload());
        let message = format!("ImgForge 启动/运行时崩溃\n\n位置: {location}\n原因: {payload}");
        let _ = write_crash_log(&message);
        show_fatal_dialog(&message);
        default_hook(info);
    }));
}

fn report_fatal(message: &str) {
    let _ = write_crash_log(message);
    show_fatal_dialog(message);
}

fn panic_payload_str(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

fn crash_log_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            let base = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            return base.join("imgforge").join("crash.log");
        }
        #[cfg(not(target_os = "windows"))]
        {
            let base = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(".imgforge").join("crash.log")
        }
    })
    .clone()
}

fn write_crash_log(message: &str) -> std::io::Result<()> {
    let path = crash_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(file, "==== {ts} ====")?;
    writeln!(file, "{message}")?;
    writeln!(file)?;
    Ok(())
}

fn show_fatal_dialog(message: &str) {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        fn wide(s: &str) -> Vec<u16> {
            OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
        }

        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(
                hwnd: *mut core::ffi::c_void,
                text: *const u16,
                caption: *const u16,
                flags: u32,
            ) -> i32;
        }

        const MB_OK: u32 = 0x0000_0000;
        const MB_ICONERROR: u32 = 0x0000_0010;
        let text = wide(message);
        let caption = wide("ImgForge");
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
        return;
    }

    #[cfg(not(windows))]
    {
        eprintln!("{message}");
        let path = crash_log_path();
        eprintln!("详情已写入 {}", path.display());
    }
}
