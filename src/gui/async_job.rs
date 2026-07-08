//! 后台任务封装：在独立线程执行并在 egui 帧循环中轮询结果。

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use eframe::egui::Context;

use crate::ui::progress::{GuiProgress, ProgressReporter};

enum JobMessage<T> {
    Finished(Result<T, String>),
}

/// 可在 UI 中轮询的后台任务。
pub struct BackgroundJob<T: Send + 'static> {
    rx: Option<Receiver<JobMessage<T>>>,
    progress: Option<Arc<GuiProgress>>,
}

impl<T: Send + 'static> Default for BackgroundJob<T> {
    fn default() -> Self {
        Self {
            rx: None,
            progress: None,
        }
    }
}

impl<T: Send + 'static> BackgroundJob<T> {
    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn progress(&self) -> Option<&Arc<GuiProgress>> {
        self.progress.as_ref()
    }

    /// 在后台线程执行任务；若已有任务在运行则忽略。
    pub fn spawn<F>(&mut self, ctx: &Context, total_hint: usize, work: F)
    where
        F: FnOnce(Arc<GuiProgress>) -> Result<T, String> + Send + 'static,
    {
        if self.is_running() {
            return;
        }
        let progress = Arc::new(GuiProgress::new());
        progress.set_total(total_hint);
        let (tx, rx) = mpsc::channel();
        let progress_worker = Arc::clone(&progress);
        thread::spawn(move || {
            let result = work(progress_worker);
            let _ = tx.send(JobMessage::Finished(result));
        });
        self.rx = Some(rx);
        self.progress = Some(progress);
        ctx.request_repaint();
    }

    /// 轮询任务状态；完成时返回 `Some(result)` 并重置为 idle。
    pub fn poll(&mut self, ctx: &Context) -> Option<Result<T, String>> {
        let Some(rx) = &self.rx else {
            return None;
        };
        match rx.try_recv() {
            Ok(JobMessage::Finished(result)) => {
                self.rx = None;
                self.progress = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint();
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.progress = None;
                Some(Err("后台任务异常中断".into()))
            }
        }
    }
}
