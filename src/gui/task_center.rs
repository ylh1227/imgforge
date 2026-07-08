//! 跨模块任务中心：统一展示历史、耗时、失败和重跑入口。

use std::path::PathBuf;

use eframe::egui::{self, RichText, ScrollArea};

use crate::gui::prefs::{ActionHistoryEntry, ActionHistoryStatus, GuiPrefs, TaskHistoryEntry};
use crate::gui::{theme, widgets};

#[derive(Debug, Clone)]
pub enum TaskCenterAction {
    LoadConvertHistory(usize),
    RetryConvertFailures(Vec<PathBuf>),
}

pub fn task_center_ui(
    ui: &mut egui::Ui,
    prefs: &GuiPrefs,
    last_failed_inputs: &[PathBuf],
    enabled: bool,
) -> Option<TaskCenterAction> {
    let dark = ui.style().visuals.dark_mode;
    let mut action = None;
    widgets::navigation_header(ui, "任务中心");
    ui.add_space(12.0);

    widgets::grouped_section(ui, "可重试队列", |ui| {
        if last_failed_inputs.is_empty() {
            ui.label(RichText::new("暂无可重试失败项").weak());
        } else {
            ui.label(format!("转换失败项：{} 个", last_failed_inputs.len()));
            for path in last_failed_inputs.iter().take(6) {
                ui.label(RichText::new(path.display().to_string()).small().weak());
            }
            if widgets::compact_primary_button(ui, "仅重试失败项", enabled).clicked() {
                action = Some(TaskCenterAction::RetryConvertFailures(
                    last_failed_inputs.to_vec(),
                ));
            }
        }
    });

    ui.add_space(10.0);
    widgets::grouped_section(ui, "转换历史", |ui| {
        if prefs.history.is_empty() {
            ui.label(RichText::new("暂无转换历史").weak());
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

    ui.add_space(10.0);
    widgets::grouped_section(ui, "模块操作日志", |ui| {
        if prefs.action_history.is_empty() {
            ui.label(RichText::new("暂无模块操作记录").weak());
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

fn convert_history_row(
    ui: &mut egui::Ui,
    idx: usize,
    entry: &TaskHistoryEntry,
    enabled: bool,
    dark: bool,
    action: &mut Option<TaskCenterAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("格式转换").strong());
        ui.label(format!(
            "{} / {} 成功，失败 {}，耗时 {}",
            entry.successes,
            entry.total,
            entry.failures,
            format_elapsed(entry.elapsed_ms)
        ));
        if widgets::compact_secondary_button(ui, "载入重跑", enabled).clicked() {
            *action = Some(TaskCenterAction::LoadConvertHistory(idx));
        }
    });
    ui.label(
        RichText::new(format!("{} → {}", entry.input_dir, entry.output_dir))
            .small()
            .color(theme::secondary_label(dark)),
    );
    ui.add_space(4.0);
}

fn action_history_row(ui: &mut egui::Ui, entry: &ActionHistoryEntry, dark: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&entry.module).strong());
        ui.label(&entry.operation);
        ui.colored_label(status_color(entry.status), entry.status.label());
        ui.label(format!(
            "成功 {} / 失败 {} / 总计 {} · {}",
            entry.success_count,
            entry.failure_count,
            entry.total_count,
            format_elapsed(entry.elapsed_ms)
        ));
    });
    ui.label(
        RichText::new(&entry.target)
            .small()
            .color(theme::secondary_label(dark)),
    );
    if let Some(detail) = &entry.detail {
        ui.collapsing("详情", |ui| {
            for line in detail.lines().take(12) {
                ui.label(RichText::new(line).small());
            }
        });
    }
    ui.add_space(4.0);
}

fn status_color(status: ActionHistoryStatus) -> egui::Color32 {
    match status {
        ActionHistoryStatus::Succeeded => egui::Color32::from_rgb(52, 199, 89),
        ActionHistoryStatus::PartiallyFailed => egui::Color32::from_rgb(255, 149, 0),
        ActionHistoryStatus::Failed => egui::Color32::from_rgb(255, 59, 48),
    }
}

fn format_elapsed(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}
