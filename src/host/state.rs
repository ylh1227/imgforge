//! Host 运行时状态：服务句柄、作业与偏好。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::prefs::GuiPrefs;
use crate::review::service::ReviewService;
use crate::ui::progress::GuiProgress;
use crate::video_review::service::VideoReviewService;

pub type EventSink = Arc<dyn Fn(crate::host::HostEvent) + Send + Sync>;

pub struct HostState {
    pub prefs: GuiPrefs,
    pub review: Option<ReviewService>,
    pub video: Option<VideoReviewService>,
    pub jobs: HashMap<String, JobRecord>,
    pub prefer_remote: bool,
    pub burn_review_annotations: bool,
}

pub struct JobRecord {
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<GuiProgress>,
    pub kind: String,
}

impl HostState {
    pub fn new() -> eyre::Result<Self> {
        let review = ReviewService::open().ok();
        let video = VideoReviewService::open().ok();
        Ok(Self {
            prefs: GuiPrefs::load(),
            review,
            video,
            jobs: HashMap::new(),
            prefer_remote: false,
            burn_review_annotations: false,
        })
    }

    pub fn save_prefs(&self) -> Result<(), String> {
        self.prefs.save().map_err(|e| e.to_string())
    }

    pub fn begin_job(&mut self, kind: &str) -> (String, Arc<AtomicBool>, Arc<GuiProgress>) {
        let job_id = uuid::Uuid::new_v4().to_string();
        let (cancel, progress) = self.begin_named_job(job_id.clone(), kind);
        (job_id, cancel, progress)
    }

    /// Register / replace a job under a stable id (e.g. `scene-recognize-{batch}`).
    /// If the same id is already running, its cancel flag is set first.
    pub fn begin_named_job(
        &mut self,
        job_id: String,
        kind: &str,
    ) -> (Arc<AtomicBool>, Arc<GuiProgress>) {
        if let Some(prev) = self.jobs.get(&job_id) {
            prev.cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(GuiProgress::new());
        self.jobs.insert(
            job_id,
            JobRecord {
                cancel: Arc::clone(&cancel),
                progress: Arc::clone(&progress),
                kind: kind.into(),
            },
        );
        (cancel, progress)
    }

    pub fn cancel_job(&self, job_id: &str) -> bool {
        if let Some(job) = self.jobs.get(job_id) {
            job.cancel.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn finish_job(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }

    pub fn review(&mut self) -> Result<&mut ReviewService, String> {
        self.review
            .as_mut()
            .ok_or_else(|| "review service unavailable".into())
    }

    pub fn video(&mut self) -> Result<&mut VideoReviewService, String> {
        self.video
            .as_mut()
            .ok_or_else(|| "video review service unavailable (check ffmpeg/db)".into())
    }
}
