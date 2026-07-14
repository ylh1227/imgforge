//! 跨模块任务中心：统一展示历史、耗时、失败、远端同步与重跑入口。

use std::path::PathBuf;

use eframe::egui::{self, RichText, ScrollArea};

use crate::gui::prefs::{ActionHistoryEntry, ActionHistoryStatus, GuiPrefs, TaskHistoryEntry};
use crate::gui::{theme, widgets};
use crate::remote::{
    RemoteConfig, RemoteJobPhase, RemoteJobSource, RemoteJobSummary, SyncSnapshot,
};

#[derive(Debug, Clone)]
pub enum TaskCenterAction {
    LoadConvertHistory(usize),
    RetryConvertFailures(Vec<PathBuf>),
    /// 从远端/缓存刷新任务列表。
    SyncRemoteJobs,
    /// 刷新单个远端任务状态。
    RefreshRemoteJob(String),
    /// 打开图片评审远端数据源。
    OpenReviewRemote,
    /// 打开视频评审远端数据源。
    OpenVideoRemote,
    /// 打开数据提取远端结果。
    OpenExtractRemote,
}

/// 任务中心远端面板所需的只读视图。
#[derive(Debug, Clone, Default)]
pub struct RemoteTaskCenterView {
    pub config_label: String,
    pub base_url: Option<String>,
    pub snapshot: Option<SyncSnapshot>,
    pub prefer_remote_execution: bool,
}

impl RemoteTaskCenterView {
    pub fn from_config(
        config: &RemoteConfig,
        prefer_remote: bool,
        snapshot: Option<&SyncSnapshot>,
    ) -> Self {
        Self {
            config_label: config.status_label().to_string(),
            base_url: config.base_url.clone(),
            snapshot: snapshot.cloned(),
            prefer_remote_execution: prefer_remote,
        }
    }
}

pub fn task_center_ui(
    ui: &mut egui::Ui,
    prefs: &GuiPrefs,
    last_failed_inputs: &[PathBuf],
    remote: &RemoteTaskCenterView,
    enabled: bool,
) -> Option<TaskCenterAction> {
    let dark = ui.style().visuals.dark_mode;
    let mut action = None;
    widgets::navigation_header(ui, "跨模块任务、远端同步与重跑入口");
    widgets::page_header_gap(ui);

    widgets::grouped_section(ui, "远端服务器", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(&remote.config_label)
                    .strong()
                    .color(theme::primary_label(dark)),
            );
            if let Some(snap) = &remote.snapshot {
                if snap.online {
                    widgets::status_badge(ui, "在线", theme::success_color(dark));
                } else {
                    widgets::status_badge(ui, "离线/缓存", theme::warning_color(dark));
                }
            }
        });

        if let Some(url) = &remote.base_url {
            ui.label(
                RichText::new(format!("URL  {url}"))
                    .size(12.5)
                    .color(theme::secondary_label(dark)),
            );
        }
        ui.label(
            RichText::new(if remote.prefer_remote_execution {
                "执行策略：优先远端（本地仍可强制本地执行）"
            } else {
                "执行策略：本地执行（默认）"
            })
            .size(12.5)
            .color(theme::secondary_label(dark)),
        );

        if let Some(snap) = &remote.snapshot {
            let sync_label = match snap.last_sync_at {
                Some(ts) => format!("最近同步  {ts}"),
                None => "最近同步  无".into(),
            };
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    if snap.from_cache { "来自缓存" } else { "实时" },
                    sync_label
                ))
                .size(12.5)
                .color(theme::secondary_label(dark)),
            );
            ui.label(
                RichText::new(&snap.health_message)
                    .size(12.5)
                    .color(theme::secondary_label(dark)),
            );
        } else {
            ui.add_space(4.0);
            widgets::empty_state(
                ui,
                "尚未同步远端任务",
                "点击下方「同步远端任务」拉取最新作业列表。",
            );
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::compact_primary_button(ui, "同步远端任务", enabled).clicked() {
                action = Some(TaskCenterAction::SyncRemoteJobs);
            }
        });

        if let Some(snap) = &remote.snapshot {
            ui.add_space(8.0);
            if snap.jobs.is_empty() {
                widgets::empty_state(ui, "暂无远端任务", "同步后，进行中与已完成的作业会显示在这里。");
            } else {
                ScrollArea::vertical()
                    .id_salt("task_center_remote_jobs")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for job in &snap.jobs {
                            remote_job_row(ui, job, enabled, dark, &mut action);
                        }
                    });
            }
        }
    });

    widgets::section_gap(ui);
    widgets::grouped_section(ui, "可重试队列", |ui| {
        if last_failed_inputs.is_empty() {
            widgets::empty_state(ui, "暂无可重试项", "转换失败后，可在此一键只重跑失败文件。");
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("{} 个失败项", last_failed_inputs.len()))
                        .strong()
                        .color(theme::primary_label(dark)),
                );
                widgets::status_badge(ui, "可重试", theme::warning_color(dark));
            });
            for path in last_failed_inputs.iter().take(6) {
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(12.5)
                        .color(theme::secondary_label(dark)),
                );
            }
            ui.add_space(4.0);
            if widgets::compact_primary_button(ui, "仅重试失败项", enabled).clicked() {
                action = Some(TaskCenterAction::RetryConvertFailures(
                    last_failed_inputs.to_vec(),
                ));
            }
        }
    });

    widgets::section_gap(ui);
    widgets::grouped_section(ui, "转换历史", |ui| {
        if prefs.history.is_empty() {
            widgets::empty_state(ui, "暂无转换历史", "完成一次批量转换后，可从这里载入参数重跑。");
        } else {
            ScrollArea::vertical()
                .id_salt("task_center_convert_history")
                .max_height(180.0)
                .show(ui, |ui| {
                    for (idx, entry) in prefs.history.iter().enumerate() {
                        convert_history_row(ui, idx, entry, enabled, dark, &mut action);
                    }
                });
        }
    });

    widgets::section_gap(ui);
    widgets::grouped_section(ui, "模块操作日志", |ui| {
        if prefs.action_history.is_empty() {
            widgets::empty_state(
                ui,
                "暂无模块操作记录",
                "评审、视频、数据提取等操作会汇总到这里。",
            );
        } else {
            ScrollArea::vertical()
                .id_salt("task_center_action_history")
                .max_height(280.0)
                .show(ui, |ui| {
                    for entry in &prefs.action_history {
                        action_history_row(ui, entry, dark);
                    }
                });
        }
    });

    action
}

fn remote_job_row(
    ui: &mut egui::Ui,
    job: &RemoteJobSummary,
    enabled: bool,
    dark: bool,
    action: &mut Option<TaskCenterAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(job.source.label())
                .strong()
                .color(theme::primary_label(dark)),
        );
        widgets::status_badge(ui, job.phase.label(), phase_color(job.phase, dark));
        if let Some(p) = job.progress {
            ui.label(
                RichText::new(format!("{:.0}%", p * 100.0))
                    .size(12.5)
                    .color(theme::secondary_label(dark)),
            );
        }
        ui.label(
            RichText::new(format!("{}/{}", job.processed, job.total))
                .size(12.5)
                .color(theme::secondary_label(dark)),
        );
        if widgets::compact_secondary_button(ui, "刷新", enabled).clicked() {
            *action = Some(TaskCenterAction::RefreshRemoteJob(job.job_id.clone()));
        }
        if job.phase == RemoteJobPhase::Succeeded {
            let open_action = match job.source {
                RemoteJobSource::Review => Some(TaskCenterAction::OpenReviewRemote),
                RemoteJobSource::VideoReview => Some(TaskCenterAction::OpenVideoRemote),
                RemoteJobSource::DataExtract => Some(TaskCenterAction::OpenExtractRemote),
                RemoteJobSource::Convert | RemoteJobSource::Other => None,
            };
            if let Some(next_action) = open_action {
                if widgets::compact_primary_button(ui, "打开", enabled).clicked() {
                    *action = Some(next_action);
                }
            }
        }
    });
    ui.label(
        RichText::new(format!("job  {}", job.job_id))
            .size(12.0)
            .color(theme::secondary_label(dark)),
    );
    if let Some(err) = &job.error_summary {
        ui.label(
            RichText::new(err)
                .size(12.5)
                .color(theme::error_color(dark)),
        );
    }
    widgets::inset_separator(ui);
}

fn convert_history_row(
    ui: &mut egui::Ui,
    idx: usize,
    entry: &TaskHistoryEntry,
    enabled: bool,
    dark: bool,
    action: &mut Option<TaskCenterAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("格式转换")
                .strong()
                .color(theme::primary_label(dark)),
        );
        let badge_color = if entry.failures == 0 {
            theme::success_color(dark)
        } else if entry.successes > 0 {
            theme::warning_color(dark)
        } else {
            theme::error_color(dark)
        };
        widgets::status_badge(
            ui,
            &format!(
                "{} / {} 成功 · 失败 {}",
                entry.successes, entry.total, entry.failures
            ),
            badge_color,
        );
        ui.label(
            RichText::new(format_elapsed(entry.elapsed_ms))
                .size(12.5)
                .color(theme::secondary_label(dark)),
        );
        if widgets::compact_secondary_button(ui, "载入重跑", enabled).clicked() {
            *action = Some(TaskCenterAction::LoadConvertHistory(idx));
        }
    });
    ui.label(
        RichText::new(format!("{} → {}", entry.input_dir, entry.output_dir))
            .size(12.5)
            .color(theme::secondary_label(dark)),
    );
    widgets::inset_separator(ui);
}

fn action_history_row(ui: &mut egui::Ui, entry: &ActionHistoryEntry, dark: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(&entry.module)
                .strong()
                .color(theme::primary_label(dark)),
        );
        ui.label(
            RichText::new(&entry.operation)
                .size(13.0)
                .color(theme::primary_label(dark)),
        );
        widgets::status_badge(ui, entry.status.label(), status_color(entry.status, dark));
        ui.label(
            RichText::new(format!(
                "成功 {} / 失败 {} / 总计 {} · {}",
                entry.success_count,
                entry.failure_count,
                entry.total_count,
                format_elapsed(entry.elapsed_ms)
            ))
            .size(12.5)
            .color(theme::secondary_label(dark)),
        );
    });
    ui.label(
        RichText::new(&entry.target)
            .size(12.5)
            .color(theme::secondary_label(dark)),
    );
    if let Some(detail) = &entry.detail {
        ui.collapsing("详情", |ui| {
            for line in detail.lines().take(12) {
                ui.label(RichText::new(line).size(12.5));
            }
        });
    }
    widgets::inset_separator(ui);
}

fn status_color(status: ActionHistoryStatus, dark: bool) -> egui::Color32 {
    match status {
        ActionHistoryStatus::Succeeded => theme::success_color(dark),
        ActionHistoryStatus::PartiallyFailed => theme::warning_color(dark),
        ActionHistoryStatus::Failed => theme::error_color(dark),
    }
}

fn phase_color(phase: RemoteJobPhase, dark: bool) -> egui::Color32 {
    match phase {
        RemoteJobPhase::Succeeded => theme::success_color(dark),
        RemoteJobPhase::Failed | RemoteJobPhase::Cancelled => theme::error_color(dark),
        RemoteJobPhase::Queued | RemoteJobPhase::Running => theme::accent(dark),
        RemoteJobPhase::Unknown => theme::info_color(dark),
    }
}

fn format_elapsed(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}
