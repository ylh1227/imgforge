//! 批量建 JIRA Issue：映射字段、准备附件、有限并发、可取消。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::jira::client::{CreatedIssue, JiraClient};
use crate::jira::config::JiraConfig;
use crate::jira::error::{JiraError, JiraResult};
use crate::jira::mapping::MappedIssue;

const CANCEL_REASON: &str = "用户取消";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JiraSubmitSource {
    ReviewImage,
    VideoDefect,
}

#[derive(Debug, Clone)]
pub struct JiraSubmitItemResult {
    pub source: JiraSubmitSource,
    pub local_id: i64,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub issue_key: Option<String>,
    pub browse_url: Option<String>,
    pub attachment_warning: Option<String>,
    /// 本地库回写失败时的提示（Issue 已建但未关联）。
    pub persist_warning: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct JiraBatchSubmitResult {
    pub items: Vec<JiraSubmitItemResult>,
}

impl JiraBatchSubmitResult {
    pub fn success_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.issue_key.is_some() && i.error.is_none() && !i.skipped)
            .count()
    }

    pub fn skipped_count(&self) -> usize {
        self.items.iter().filter(|i| i.skipped).count()
    }

    pub fn cancelled_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| {
                i.skipped
                    && i.skip_reason
                        .as_deref()
                        .is_some_and(|r| r == CANCEL_REASON)
            })
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.items.iter().filter(|i| i.error.is_some()).count()
    }

    pub fn summary_line(&self) -> String {
        let cancelled = self.cancelled_count();
        if cancelled > 0 {
            format!(
                "JIRA 提交完成：成功 {} · 跳过 {} · 取消后未提交 {} · 失败 {}",
                self.success_count(),
                self.skipped_count().saturating_sub(cancelled),
                cancelled,
                self.failed_count()
            )
        } else {
            format!(
                "JIRA 提交完成：成功 {} · 跳过 {} · 失败 {}",
                self.success_count(),
                self.skipped_count(),
                self.failed_count()
            )
        }
    }
}

#[derive(Debug, Clone)]
pub struct JiraBatchOptions {
    /// 已有 jira_issue_key 时仍强制新建。
    pub force_recreate: bool,
    /// 是否上传附件（截图 / zip）。
    pub attach: bool,
}

impl Default for JiraBatchOptions {
    fn default() -> Self {
        Self {
            force_recreate: false,
            attach: true,
        }
    }
}

pub type ProgressFn<'a> = dyn Fn(usize, usize, &str) + 'a;

pub struct JiraIssueService {
    client: JiraClient,
}

struct PreparedWork {
    index: usize,
    local_id: i64,
    source: JiraSubmitSource,
    mapped: MappedIssue,
    attachment: Option<PathBuf>,
    warn_missing_attach: Option<String>,
}

impl JiraIssueService {
    pub fn try_new(config: &JiraConfig) -> JiraResult<Self> {
        Ok(Self {
            client: JiraClient::try_new(config)?,
        })
    }

    pub fn client(&self) -> &JiraClient {
        &self.client
    }

    fn concurrency(&self) -> usize {
        self.client.config().effective_concurrency()
    }

    #[cfg(feature = "review")]
    pub fn batch_create_from_review_items(
        &self,
        items: &[crate::review::domain::image_item::ReviewImageItem],
        attachments: &[(i64, PathBuf)],
        annotation_summaries: &[(i64, String)],
        options: &JiraBatchOptions,
        progress: Option<&ProgressFn<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> JiraResult<JiraBatchSubmitResult> {
        let total = items.len();
        let mut slots: Vec<Option<JiraSubmitItemResult>> = vec![None; total];
        let mut pending = Vec::new();

        for (idx, item) in items.iter().enumerate() {
            if is_cancelled(cancel) {
                fill_cancelled(
                    &mut slots,
                    idx..total,
                    JiraSubmitSource::ReviewImage,
                    items,
                    |i| i.id,
                );
                break;
            }
            if !options.force_recreate {
                if let Some(key) = item.jira_issue_key.as_ref().filter(|k| !k.is_empty()) {
                    slots[idx] = Some(JiraSubmitItemResult {
                        source: JiraSubmitSource::ReviewImage,
                        local_id: item.id,
                        skipped: true,
                        skip_reason: Some(format!("已关联 {key}")),
                        issue_key: Some(key.clone()),
                        browse_url: item.jira_url.clone(),
                        attachment_warning: None,
                        persist_warning: None,
                        error: None,
                    });
                    if let Some(cb) = progress {
                        cb(idx + 1, total, &format!("跳过已关联 · #{}" , item.id));
                    }
                    continue;
                }
            }
            let ann = annotation_summaries
                .iter()
                .find(|(id, _)| *id == item.id)
                .map(|(_, s)| s.as_str());
            let mapped = crate::jira::mapping::map_review_item_with_annotations(
                self.client.config(),
                item,
                ann,
            );
            pending.push(PreparedWork {
                index: idx,
                local_id: item.id,
                source: JiraSubmitSource::ReviewImage,
                mapped,
                attachment: attachment_for(attachments, item.id),
                warn_missing_attach: None,
            });
        }

        self.run_prepared(pending, &mut slots, total, options, progress, cancel)?;
        Ok(JiraBatchSubmitResult {
            items: slots.into_iter().flatten().collect(),
        })
    }

    #[cfg(feature = "video-review")]
    pub fn batch_create_from_defects(
        &self,
        defects: &[crate::video_review::domain::VideoDefect],
        options: &JiraBatchOptions,
        progress: Option<&ProgressFn<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> JiraResult<JiraBatchSubmitResult> {
        let total = defects.len();
        let mut slots: Vec<Option<JiraSubmitItemResult>> = vec![None; total];
        let mut pending = Vec::new();

        for (idx, defect) in defects.iter().enumerate() {
            if is_cancelled(cancel) {
                fill_cancelled(
                    &mut slots,
                    idx..total,
                    JiraSubmitSource::VideoDefect,
                    defects,
                    |d| d.id,
                );
                break;
            }
            if !options.force_recreate {
                if let Some(key) = defect.jira_issue_key.as_ref().filter(|k| !k.is_empty()) {
                    slots[idx] = Some(JiraSubmitItemResult {
                        source: JiraSubmitSource::VideoDefect,
                        local_id: defect.id,
                        skipped: true,
                        skip_reason: Some(format!("已关联 {key}")),
                        issue_key: Some(key.clone()),
                        browse_url: defect.jira_url.clone(),
                        attachment_warning: None,
                        persist_warning: None,
                        error: None,
                    });
                    if let Some(cb) = progress {
                        cb(idx + 1, total, &format!("跳过已关联 · {}", defect.title));
                    }
                    continue;
                }
            }

            let manifest = crate::jira::mapping::try_load_defect_manifest_summary(
                defect.package_path.as_deref(),
            );
            let mapped = crate::jira::mapping::map_video_defect_with_manifest(
                self.client.config(),
                defect,
                manifest.as_deref(),
            );
            let attach_path = if options.attach && self.client.config().attach_defect_zip {
                defect
                    .package_path
                    .as_ref()
                    .filter(|p| p.exists())
                    .cloned()
            } else {
                None
            };
            let warn_missing = if options.attach
                && self.client.config().attach_defect_zip
                && attach_path.is_none()
            {
                Some("无缺陷包 zip，已跳过附件".to_string())
            } else {
                None
            };
            pending.push(PreparedWork {
                index: idx,
                local_id: defect.id,
                source: JiraSubmitSource::VideoDefect,
                mapped,
                attachment: attach_path,
                warn_missing_attach: warn_missing,
            });
        }

        self.run_prepared(pending, &mut slots, total, options, progress, cancel)?;
        Ok(JiraBatchSubmitResult {
            items: slots.into_iter().flatten().collect(),
        })
    }

    fn run_prepared(
        &self,
        pending: Vec<PreparedWork>,
        slots: &mut [Option<JiraSubmitItemResult>],
        total: usize,
        options: &JiraBatchOptions,
        progress: Option<&ProgressFn<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> JiraResult<()> {
        if pending.is_empty() {
            return Ok(());
        }
        if is_cancelled(cancel) {
            for w in pending {
                slots[w.index] = Some(cancelled_item(w.source, w.local_id));
            }
            return Ok(());
        }

        let concurrency = self.concurrency();
        if concurrency <= 1 {
            return self.run_serial(pending, slots, total, options, progress, cancel);
        }
        self.run_concurrent(pending, slots, total, options, progress, cancel, concurrency)
    }

    fn run_serial(
        &self,
        pending: Vec<PreparedWork>,
        slots: &mut [Option<JiraSubmitItemResult>],
        total: usize,
        options: &JiraBatchOptions,
        progress: Option<&ProgressFn<'_>>,
        cancel: Option<&AtomicBool>,
    ) -> JiraResult<()> {
        let mut done = slots.iter().filter(|s| s.is_some()).count();
        let mut iter = pending.into_iter();
        while let Some(work) = iter.next() {
            if is_cancelled(cancel) {
                slots[work.index] = Some(cancelled_item(work.source, work.local_id));
                for rest in iter {
                    slots[rest.index] = Some(cancelled_item(rest.source, rest.local_id));
                }
                break;
            }
            done += 1;
            if let Some(cb) = progress {
                cb(
                    done,
                    total,
                    &format!("创建 Issue {done}/{total} · #{}", work.local_id),
                );
            }
            match self.create_one(work.mapped, work.attachment, options) {
                Ok((created, warn)) => {
                    slots[work.index] = Some(JiraSubmitItemResult {
                        source: work.source,
                        local_id: work.local_id,
                        skipped: false,
                        skip_reason: None,
                        issue_key: Some(created.key),
                        browse_url: created.browse_url,
                        attachment_warning: warn.or(work.warn_missing_attach),
                        persist_warning: None,
                        error: None,
                    });
                }
                Err(e) if e.is_auth_failure() => {
                    let msg = e.to_string();
                    slots[work.index] =
                        Some(failed_item(work.source, work.local_id, msg.clone()));
                    for rest in iter {
                        slots[rest.index] =
                            Some(failed_item(rest.source, rest.local_id, msg.clone()));
                    }
                    break;
                }
                Err(e) => {
                    slots[work.index] =
                        Some(failed_item(work.source, work.local_id, e.to_string()));
                }
            }
        }
        Ok(())
    }

    fn run_concurrent(
        &self,
        pending: Vec<PreparedWork>,
        slots: &mut [Option<JiraSubmitItemResult>],
        total: usize,
        options: &JiraBatchOptions,
        progress: Option<&ProgressFn<'_>>,
        cancel: Option<&AtomicBool>,
        concurrency: usize,
    ) -> JiraResult<()> {
        let queue = Arc::new(Mutex::new(VecDeque::from(pending)));
        let results: Arc<Mutex<Vec<(usize, JiraSubmitItemResult)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let completed = Arc::new(AtomicUsize::new(slots.iter().filter(|s| s.is_some()).count()));
        let auth_abort = Arc::new(AtomicBool::new(false));
        // Workers mirror the caller's cancel flag via this Arc (polled on the main thread).
        let cancel_shared = Arc::new(AtomicBool::new(is_cancelled(cancel)));

        let options = options.clone();
        let mut handles = Vec::new();
        for _ in 0..concurrency {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let completed = Arc::clone(&completed);
            let auth_abort = Arc::clone(&auth_abort);
            let cancel_shared = Arc::clone(&cancel_shared);
            let client = self.client.clone();
            let options = options.clone();
            handles.push(thread::spawn(move || {
                let svc = JiraIssueService { client };
                loop {
                    if cancel_shared.load(Ordering::Relaxed) || auth_abort.load(Ordering::Relaxed)
                    {
                        break;
                    }
                    let work = {
                        let mut q = queue.lock().unwrap();
                        q.pop_front()
                    };
                    let Some(work) = work else { break };
                    if cancel_shared.load(Ordering::Relaxed) {
                        results.lock().unwrap().push((
                            work.index,
                            cancelled_item(work.source, work.local_id),
                        ));
                        continue;
                    }
                    if auth_abort.load(Ordering::Relaxed) {
                        results.lock().unwrap().push((
                            work.index,
                            failed_item(work.source, work.local_id, "认证失败，已中止".into()),
                        ));
                        continue;
                    }
                    completed.fetch_add(1, Ordering::Relaxed);
                    match svc.create_one(work.mapped, work.attachment, &options) {
                        Ok((created, warn)) => {
                            results.lock().unwrap().push((
                                work.index,
                                JiraSubmitItemResult {
                                    source: work.source,
                                    local_id: work.local_id,
                                    skipped: false,
                                    skip_reason: None,
                                    issue_key: Some(created.key),
                                    browse_url: created.browse_url,
                                    attachment_warning: warn.or(work.warn_missing_attach),
                                    persist_warning: None,
                                    error: None,
                                },
                            ));
                        }
                        Err(e) if e.is_auth_failure() => {
                            auth_abort.store(true, Ordering::Relaxed);
                            results.lock().unwrap().push((
                                work.index,
                                failed_item(work.source, work.local_id, e.to_string()),
                            ));
                        }
                        Err(e) => {
                            results.lock().unwrap().push((
                                work.index,
                                failed_item(work.source, work.local_id, e.to_string()),
                            ));
                        }
                    }
                }
            }));
        }

        // Main thread: poll cancel into cancel_shared + progress
        while handles.iter().any(|h| !h.is_finished()) {
            if is_cancelled(cancel) {
                cancel_shared.store(true, Ordering::Relaxed);
            }
            if let Some(cb) = progress {
                let done = completed.load(Ordering::Relaxed);
                cb(done.min(total), total, &format!("创建 Issue {done}/{total}"));
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
        for h in handles {
            let _ = h.join();
        }
        // Drain leftover queue as cancelled / auth abort
        {
            let mut q = queue.lock().unwrap();
            let auth = auth_abort.load(Ordering::Relaxed);
            while let Some(work) = q.pop_front() {
                let item = if auth {
                    failed_item(work.source, work.local_id, "认证失败，已中止".into())
                } else {
                    cancelled_item(work.source, work.local_id)
                };
                results.lock().unwrap().push((work.index, item));
            }
        }
        for (idx, item) in results.lock().unwrap().drain(..) {
            slots[idx] = Some(item);
        }
        if let Some(cb) = progress {
            let done = slots.iter().filter(|s| s.is_some()).count();
            cb(done, total, "完成");
        }
        Ok(())
    }

    fn create_one(
        &self,
        mapped: MappedIssue,
        attachment: Option<PathBuf>,
        options: &JiraBatchOptions,
    ) -> JiraResult<(CreatedIssue, Option<String>)> {
        let created = self.client.create_issue(&mapped)?;
        let mut warn = None;
        if options.attach {
            if let Some(path) = attachment {
                match self.client.attach_file(&created.key, &path) {
                    Ok(()) => {}
                    Err(JiraError::AttachmentTooLarge { path, size, limit }) => {
                        warn = Some(format!(
                            "附件过大已跳过（{path}，{size}/{limit} bytes）"
                        ));
                    }
                    Err(e) => {
                        warn = Some(format!("附件上传失败：{e}"));
                    }
                }
            }
        }
        Ok((created, warn))
    }
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel
        .map(|c| c.load(Ordering::Relaxed))
        .unwrap_or(false)
}

fn cancelled_item(source: JiraSubmitSource, local_id: i64) -> JiraSubmitItemResult {
    JiraSubmitItemResult {
        source,
        local_id,
        skipped: true,
        skip_reason: Some(CANCEL_REASON.into()),
        issue_key: None,
        browse_url: None,
        attachment_warning: None,
        persist_warning: None,
        error: None,
    }
}

fn fill_cancelled<T>(
    slots: &mut [Option<JiraSubmitItemResult>],
    range: std::ops::Range<usize>,
    source: JiraSubmitSource,
    items: &[T],
    id_of: impl Fn(&T) -> i64,
) {
    for idx in range {
        if slots[idx].is_none() {
            let id = items.get(idx).map(|i| id_of(i)).unwrap_or(0);
            slots[idx] = Some(cancelled_item(source, id));
        }
    }
}

fn attachment_for(attachments: &[(i64, PathBuf)], id: i64) -> Option<PathBuf> {
    attachments
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, p)| p.clone())
}

fn failed_item(source: JiraSubmitSource, local_id: i64, error: String) -> JiraSubmitItemResult {
    JiraSubmitItemResult {
        source,
        local_id,
        skipped: false,
        skip_reason: None,
        issue_key: None,
        browse_url: None,
        attachment_warning: None,
        persist_warning: None,
        error: Some(error),
    }
}

/// 将截图导出结果整理为 (image_id, path) 列表。
pub fn screenshot_attachments_from_manifest(entries: &[(i64, PathBuf)]) -> Vec<(i64, PathBuf)> {
    entries
        .iter()
        .filter(|(_, p)| p.exists())
        .cloned()
        .collect()
}

pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(all(test, feature = "video-review"))]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use chrono::Utc;
    use httpmock::prelude::*;

    use crate::jira::config::{JiraAuthMode, JiraConfig};
    use crate::video_review::domain::VideoDefect;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_basic_creds<R>(f: impl FnOnce() -> R) -> R {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("IMGFORGE_JIRA_EMAIL", "tester@example.com");
        std::env::set_var("IMGFORGE_JIRA_API_TOKEN", "test-token");
        f()
    }

    fn test_cfg(server: &MockServer, max_concurrent: u32) -> JiraConfig {
        JiraConfig {
            enabled: true,
            base_url: Some(server.base_url()),
            project_key: Some("CAM".into()),
            auth_mode: JiraAuthMode::EnvBasic,
            timeout_secs: 10,
            max_concurrent,
            attach_defect_zip: false,
            ..JiraConfig::default()
        }
    }

    fn sample_defect(id: i64) -> VideoDefect {
        VideoDefect {
            id,
            batch_id: 1,
            title: format!("defect-{id}"),
            description: "desc".into(),
            severity: 3,
            time_ms: 1000,
            half_window_ms: 500,
            video_ids: vec![1],
            package_path: None,
            created_at: Utc::now(),
            jira_issue_key: None,
            jira_url: None,
        }
    }

    #[test]
    fn cancel_skips_unstarted_items() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(201)
                    .delay(Duration::from_millis(200))
                    .json_body(serde_json::json!({
                        "id": "1",
                        "key": "CAM-1",
                        "self": format!("{}/rest/api/3/issue/1", server.base_url())
                    }));
            });
            let service = JiraIssueService::try_new(&test_cfg(&server, 1)).unwrap();
            let defects: Vec<_> = (1..=4).map(sample_defect).collect();
            let cancel = AtomicBool::new(false);
            // Cancel before any create starts
            cancel.store(true, Ordering::Relaxed);
            let result = service
                .batch_create_from_defects(
                    &defects,
                    &JiraBatchOptions {
                        force_recreate: false,
                        attach: false,
                    },
                    None,
                    Some(&cancel),
                )
                .unwrap();
            assert_eq!(result.cancelled_count(), 4);
            assert_eq!(result.success_count(), 0);
            assert_eq!(mock.hits(), 0);
            assert!(result.summary_line().contains("取消后未提交 4"));
        });
    }

    #[test]
    fn cancel_mid_batch_skips_remaining_serial() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(201).json_body(serde_json::json!({
                    "id": "1",
                    "key": "CAM-1",
                    "self": "http://localhost/rest/api/3/issue/1"
                }));
            });
            let service = JiraIssueService::try_new(&test_cfg(&server, 1)).unwrap();
            let defects: Vec<_> = (1..=3).map(sample_defect).collect();
            let cancel = Arc::new(AtomicBool::new(false));
            let cancel_for_progress = Arc::clone(&cancel);
            let result = service
                .batch_create_from_defects(
                    &defects,
                    &JiraBatchOptions {
                        force_recreate: false,
                        attach: false,
                    },
                    Some(&|done, _tot, _label| {
                        if done >= 1 {
                            cancel_for_progress.store(true, Ordering::Relaxed);
                        }
                    }),
                    Some(cancel.as_ref()),
                )
                .unwrap();
            assert!(result.success_count() >= 1);
            assert!(result.cancelled_count() >= 1);
            assert_eq!(result.items.len(), 3);
            assert_eq!(
                result.success_count() + result.cancelled_count() + result.failed_count(),
                3
            );
            assert!(mock.hits() >= 1);
            assert!(mock.hits() < 3);
        });
    }

    #[test]
    fn concurrent_create_overlaps_requests() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(201)
                    .delay(Duration::from_millis(150))
                    .json_body(serde_json::json!({
                        "id": "1",
                        "key": "CAM-9",
                        "self": "http://localhost/rest/api/3/issue/1"
                    }));
            });

            // Wall time: 4 serial delayed creates ≈ 600ms; concurrent 2 ≈ ~300ms.
            let service = JiraIssueService::try_new(&test_cfg(&server, 2)).unwrap();
            let defects: Vec<_> = (1..=4).map(sample_defect).collect();
            let started = Instant::now();
            let result = service
                .batch_create_from_defects(
                    &defects,
                    &JiraBatchOptions {
                        force_recreate: false,
                        attach: false,
                    },
                    None,
                    None,
                )
                .unwrap();
            let elapsed = started.elapsed();
            assert_eq!(result.success_count(), 4);
            assert_eq!(mock.hits(), 4);
            assert!(
                elapsed < Duration::from_millis(500),
                "expected concurrent overlap, elapsed={elapsed:?}"
            );
            assert_eq!(
                result.items.iter().map(|i| i.local_id).collect::<Vec<_>>(),
                vec![1, 2, 3, 4]
            );
        });
    }

    #[test]
    fn skip_already_linked() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(201).json_body(serde_json::json!({
                    "id": "1",
                    "key": "CAM-2",
                    "self": "http://localhost/rest/api/3/issue/1"
                }));
            });
            let service = JiraIssueService::try_new(&test_cfg(&server, 1)).unwrap();
            let mut d = sample_defect(1);
            d.jira_issue_key = Some("CAM-99".into());
            d.jira_url = Some("http://localhost/browse/CAM-99".into());
            let result = service
                .batch_create_from_defects(
                    &[d],
                    &JiraBatchOptions {
                        force_recreate: false,
                        attach: false,
                    },
                    None,
                    None,
                )
                .unwrap();
            assert_eq!(result.skipped_count(), 1);
            assert_eq!(mock.hits(), 0);
        });
    }
}

