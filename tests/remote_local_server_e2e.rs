//! 远端栈开发后备端到端：SQLite/磁盘/内存队列下上传 → 转换 → 下载。

#![cfg(feature = "server")]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use image::{Rgb, RgbImage};
use imgforge::config::AppConfig;
use imgforge::core::types::ImageFormat;
use imgforge::remote::config::RemoteConfig;
use imgforge::remote::{try_build_http_client, RemoteAuthMode, RemoteJobPhase, TaskSyncService};
use imgforge::server::{api, worker::InlineWorker, AppState, ServerConfig};
use tempfile::tempdir;

#[test]
fn remote_convert_png_to_webp_end_to_end() {
    let data = tempdir().unwrap();
    let work = tempdir().unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let data_dir = data.path().to_path_buf();
    let public_base = format!("http://{addr}");
    let bind = addr.to_string();

    let server_thread = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut cfg = ServerConfig::default();
            cfg.data_dir = data_dir;
            cfg.bind = bind.clone();
            cfg.public_base = public_base;
            cfg.inline_worker = true;
            let state = AppState::local_disk(cfg).unwrap();
            let _worker = InlineWorker::spawn(state.clone());
            let app = api::app(state);
            let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
            let _ = ready_tx.send(());
            axum::serve(listener, app).await.unwrap();
        });
    });

    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server failed to start");
    thread::sleep(Duration::from_millis(100));

    let input = work.path().join("in");
    let output = work.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    let png = input.join("sample.png");
    let img = RgbImage::from_pixel(16, 16, Rgb([40, 80, 120]));
    image::DynamicImage::ImageRgb8(img).save(&png).unwrap();

    let remote = RemoteConfig {
        enabled: true,
        base_url: Some(format!("http://{addr}")),
        auth_mode: RemoteAuthMode::None,
        timeout_secs: 30,
        offline_cache: false,
        ..RemoteConfig::default()
    };
    let client = try_build_http_client(&remote).unwrap();
    let sync = TaskSyncService::new(remote, client);

    let mut app_cfg = AppConfig::default();
    app_cfg.input_dir = input;
    app_cfg.output_dir = output;
    app_cfg.explicit_inputs = vec![png];
    app_cfg.target_format = ImageFormat::WebP;
    app_cfg.overwrite = true;

    let outcome = sync.run_convert_and_download(&app_cfg, None).unwrap();

    assert_eq!(outcome.status.phase, RemoteJobPhase::Succeeded);
    assert_eq!(outcome.result.successes, 1);
    assert_eq!(outcome.downloaded.len(), 1);
    assert!(outcome.downloaded[0].exists());
    assert_eq!(
        outcome.downloaded[0].extension().and_then(|e| e.to_str()),
        Some("webp")
    );

    // 不 join 服务器线程（会一直 serve）；进程结束即清理。
    let _ = server_thread.thread().id();
}
