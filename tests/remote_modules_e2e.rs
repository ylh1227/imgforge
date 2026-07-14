//! Remote module E2E over the server feature using the SQLite/disk/in-memory fallback stack.

#![cfg(feature = "server")]

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use image::{Rgb, RgbImage};
use imgforge::remote::catalog::{
    RemoteExtractResultSummary, RemotePage, RemotePageQuery, RemoteReviewBatchSummary,
};
use imgforge::remote::models::RemoteReviewItem;
use imgforge::remote::types::{
    RemoteAssetRef, RemoteJobParams, RemoteJobPhase, RemoteJobRequest, RemoteJobResult,
    RemoteJobSource,
};
use imgforge::remote::upload::{
    RemoteUploadCompleteRequest, RemoteUploadCompleteResponse, RemoteUploadInitRequest,
    RemoteUploadSession,
};
use imgforge::remote::{HttpRemoteClient, RemoteClient, RemoteConfig};
use imgforge::server::{api, worker::InlineWorker, AppState, ServerConfig};
use reqwest::blocking::{Client, Response};
use tempfile::{tempdir, TempDir};

struct TestServer {
    base_url: String,
    _data: TempDir,
    _thread: thread::JoinHandle<()>,
}

#[test]
fn remote_modules_convert_review_video_extract_end_to_end() {
    let server = start_server();
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let remote_client = HttpRemoteClient::try_new(&RemoteConfig {
        enabled: true,
        base_url: Some(server.base_url.clone()),
        ..RemoteConfig::default()
    })
    .unwrap();

    let convert_asset = upload_asset(
        &client,
        &server.base_url,
        "convert.png",
        Some("image/png"),
        &png_bytes(),
    );
    let convert = submit_job(
        &client,
        &server.base_url,
        RemoteJobSource::Convert,
        vec![convert_asset],
        RemoteJobParams {
            target_format: Some("webp".into()),
            overwrite: Some(true),
            ..Default::default()
        },
        "convert-e2e",
    );
    let convert_result = wait_for_result(&client, &server.base_url, &convert.job_id);
    assert_eq!(convert_result.phase, RemoteJobPhase::Succeeded);
    assert_eq!(convert_result.successes, 1);
    assert!(convert_result
        .artifacts
        .iter()
        .any(|asset| asset.name.ends_with(".webp")));

    let review_asset = upload_asset(
        &client,
        &server.base_url,
        "review.png",
        Some("image/png"),
        &png_bytes(),
    );
    let review = submit_job(
        &client,
        &server.base_url,
        RemoteJobSource::Review,
        vec![review_asset],
        RemoteJobParams {
            extras: vec![("batch_name".into(), "Review E2E".into())],
            ..Default::default()
        },
        "review-e2e",
    );
    let review_result = wait_for_result(&client, &server.base_url, &review.job_id);
    assert_eq!(review_result.phase, RemoteJobPhase::Succeeded);
    let review_batches: RemotePage<RemoteReviewBatchSummary> =
        get_json(&client, &format!("{}/v1/review/batches", server.base_url));
    let review_batch = review_batches
        .items
        .iter()
        .find(|batch| batch.name == "Review E2E")
        .expect("review batch should be listed");
    let review_items: serde_json::Value = get_json(
        &client,
        &format!(
            "{}/v1/review/batches/{}/items",
            server.base_url, review_batch.batch_id
        ),
    );
    let items: Vec<RemoteReviewItem> =
        serde_json::from_value(review_items["items"].clone()).unwrap();
    assert_eq!(items.len(), 1);
    let sdk_items = remote_client
        .list_review_items(&review_batch.batch_id)
        .unwrap();
    assert_eq!(sdk_items.len(), 1);
    if let Some(thumb) = sdk_items.first().and_then(|item| item.thumb_asset.as_ref()) {
        let cred = remote_client.artifact_download_url(&thumb.id).unwrap();
        let bytes = remote_client.download_bytes(&cred.download_url).unwrap();
        assert!(!bytes.is_empty());
    }

    let video_asset = upload_asset(
        &client,
        &server.base_url,
        "placeholder.mp4",
        Some("video/mp4"),
        b"not a real video",
    );
    let video = submit_job(
        &client,
        &server.base_url,
        RemoteJobSource::VideoReview,
        vec![video_asset],
        RemoteJobParams {
            extras: vec![("batch_name".into(), "Video E2E".into())],
            ..Default::default()
        },
        "video-e2e",
    );
    let video_result = wait_for_result(&client, &server.base_url, &video.job_id);
    assert_eq!(video_result.phase, RemoteJobPhase::Succeeded);
    assert!(video_result
        .artifacts
        .iter()
        .any(|asset| asset.name.ends_with("-metadata.json")));
    let video_batches: RemotePage<RemoteReviewBatchSummary> =
        get_json(&client, &format!("{}/v1/video/batches", server.base_url));
    assert!(video_batches
        .items
        .iter()
        .any(|batch| batch.name == "Video E2E"));

    let extract_asset = upload_asset(
        &client,
        &server.base_url,
        "sample.csv",
        Some("text/csv"),
        b"name,value\nalpha,1\n",
    );
    let extract = submit_job(
        &client,
        &server.base_url,
        RemoteJobSource::DataExtract,
        vec![extract_asset],
        RemoteJobParams {
            extras: vec![("batch_name".into(), "Extract E2E".into())],
            ..Default::default()
        },
        "extract-e2e",
    );
    let extract_result = wait_for_result(&client, &server.base_url, &extract.job_id);
    assert_eq!(extract_result.phase, RemoteJobPhase::Succeeded);
    let extract_results: RemotePage<RemoteExtractResultSummary> =
        get_json(&client, &format!("{}/v1/extract/results", server.base_url));
    let summary = extract_results
        .items
        .iter()
        .find(|result| result.batch_name == "Extract E2E")
        .expect("extract result should be listed");
    let sdk_extract_results = remote_client
        .list_extract_results(RemotePageQuery {
            limit: 200,
            ..RemotePageQuery::default()
        })
        .unwrap();
    assert!(sdk_extract_results
        .items
        .iter()
        .any(|result| result.batch_name == "Extract E2E"));
    let report = summary
        .report_asset
        .as_ref()
        .expect("extract result should expose report artifact");
    let report_cred = remote_client.artifact_download_url(&report.id).unwrap();
    let report_bytes = remote_client
        .download_bytes(&report_cred.download_url)
        .unwrap();
    assert!(!report_bytes.is_empty());
    let report_text = String::from_utf8(report_bytes).unwrap();
    assert!(report_text.contains("asset_id,name,size,mime"));
}

fn start_server() -> TestServer {
    let data = tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let data_dir = data.path().to_path_buf();
    let public_base = format!("http://{addr}");
    let bind = addr.to_string();

    let server_bind = bind.clone();
    let thread = thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut cfg = ServerConfig::default();
            cfg.data_dir = data_dir;
            cfg.bind = server_bind.clone();
            cfg.public_base = public_base;
            cfg.inline_worker = true;
            cfg.rate_limit_per_minute = 1_000;
            let state = AppState::local_disk(cfg).unwrap();
            let _worker = InlineWorker::spawn(state.clone());
            let app = api::app(state);
            let listener = tokio::net::TcpListener::bind(&server_bind).await.unwrap();
            let _ = ready_tx.send(());
            axum::serve(listener, app).await.unwrap();
        });
    });

    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server failed to start");
    thread::sleep(Duration::from_millis(100));

    TestServer {
        base_url: format!("http://{addr}"),
        _data: data,
        _thread: thread,
    }
}

fn upload_asset(
    client: &Client,
    base_url: &str,
    file_name: &str,
    mime: Option<&str>,
    bytes: &[u8],
) -> RemoteAssetRef {
    let session: RemoteUploadSession = ok(client
        .post(format!("{base_url}/v1/uploads:init"))
        .json(&RemoteUploadInitRequest {
            file_name: file_name.into(),
            mime: mime.map(ToOwned::to_owned),
            size: Some(bytes.len() as u64),
            ..Default::default()
        })
        .send()
        .unwrap())
    .json()
    .unwrap();

    let upload_url = &session.parts.first().expect("single PUT part").upload_url;
    ok(client.put(upload_url).body(bytes.to_vec()).send().unwrap());

    let complete: RemoteUploadCompleteResponse = ok(client
        .post(format!("{base_url}/v1/uploads:complete"))
        .json(&RemoteUploadCompleteRequest {
            upload_id: session.upload_id,
            ..Default::default()
        })
        .send()
        .unwrap())
    .json()
    .unwrap();
    complete.asset
}

fn submit_job(
    client: &Client,
    base_url: &str,
    source: RemoteJobSource,
    inputs: Vec<RemoteAssetRef>,
    params: RemoteJobParams,
    client_request_id: &str,
) -> imgforge::remote::types::RemoteJobStatus {
    ok(client
        .post(format!("{base_url}/v1/jobs"))
        .json(&RemoteJobRequest {
            source,
            inputs,
            params,
            client_request_id: Some(client_request_id.into()),
            ..Default::default()
        })
        .send()
        .unwrap())
    .json()
    .unwrap()
}

fn wait_for_result(client: &Client, base_url: &str, job_id: &str) -> RemoteJobResult {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status: imgforge::remote::types::RemoteJobStatus =
            get_json(client, &format!("{base_url}/v1/jobs/{job_id}"));
        if status.phase.is_terminal() {
            assert_eq!(
                status.phase,
                RemoteJobPhase::Succeeded,
                "job failed: {:?}",
                status.error_summary
            );
            return get_json(client, &format!("{base_url}/v1/jobs/{job_id}/result"));
        }
        assert!(Instant::now() < deadline, "job {job_id} timed out");
        thread::sleep(Duration::from_millis(100));
    }
}

fn get_json<T: serde::de::DeserializeOwned>(client: &Client, url: &str) -> T {
    ok(client.get(url).send().unwrap()).json().unwrap()
}

fn ok(response: Response) -> Response {
    if response.status().is_success() {
        response
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        panic!("HTTP {status}: {body}");
    }
}

fn png_bytes() -> Vec<u8> {
    let image = RgbImage::from_pixel(16, 16, Rgb([40, 80, 120]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}
