//! 数据提取 GUI 面板。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, Color32, Context, CornerRadius, Frame, Margin, RichText, ScrollArea};
use serde::{Deserialize, Serialize};

use crate::data_extract::domain::{
    BatchComparison, EvaluationStatus, ExtractionBatch, ExtractionRecord, ImatestModule,
    SummaryTable, ThresholdProfile,
};
use crate::data_extract::service::{
    CompareService, DataExportService, DataExtractService, DataInsightReport, DataInsightService,
    DataQueryService, SummaryService, TableExportSchema, ThresholdService,
};
use crate::gui::prefs::{self, ActionHistoryEntry, ActionHistoryStatus, ExportTemplate, GuiPrefs};
use crate::gui::{theme, widgets};

#[cfg(feature = "ocr")]
use crate::data_extract::ocr::check_availability;

#[derive(Debug, Clone, Default)]
pub struct DataExtractPanelOutput {
    pub status_message: String,
}

pub struct DataExtractPanel {
    batches: Vec<ExtractionBatch>,
    current_batch: Option<usize>,
    baseline_batch: Option<usize>,
    selected_record: Option<usize>,
    module_filter: Option<ImatestModule>,
    status_filter: Option<EvaluationStatus>,
    search_buf: String,
    threshold_profile: ThresholdProfile,
    comparison: Option<BatchComparison>,
    table_view: DataTableView,
    action_history: Vec<ActionHistoryEntry>,
    summary_cache: Option<SummaryTable>,
    summary_dirty: bool,
    export_column_keys: Vec<String>,
    export_columns_initialized: bool,
    export_template_name: String,
    selected_export_cell: Option<EditableCellKey>,
    edit_cell_buf: String,
    export_overrides: BTreeMap<String, String>,
    batch_rename_buf: String,
    pending_delete_batch: Option<usize>,
    error: Option<String>,
    status_hint: String,
    output: DataExtractPanelOutput,
    remote_config: crate::remote::RemoteConfig,
    data_source: crate::remote::DataSource,
    remote_results: Vec<crate::remote::RemoteExtractResultSummary>,
    results_fetch:
        Option<crate::remote::RemoteFetch<Vec<crate::remote::RemoteExtractResultSummary>>>,
    report_fetch: Option<crate::remote::RemoteFetch<(String, std::path::PathBuf)>>,
    remote_loading: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EditableCellKey {
    row_key: String,
    column_key: String,
    label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataExtractDraft {
    batches: Vec<ExtractionBatch>,
    current_batch: Option<usize>,
    baseline_batch: Option<usize>,
    export_column_keys: Vec<String>,
    export_overrides: BTreeMap<String, String>,
}

impl EditableCellKey {
    fn storage_key(&self) -> String {
        format!("{}::{}", self.row_key, self.column_key)
    }
}

#[derive(Debug, Clone, Default)]
struct BatchActionResult {
    total: usize,
    successes: usize,
    failures: Vec<String>,
}

impl BatchActionResult {
    fn status(&self) -> ActionHistoryStatus {
        if self.failures.is_empty() {
            ActionHistoryStatus::Succeeded
        } else if self.successes > 0 {
            ActionHistoryStatus::PartiallyFailed
        } else {
            ActionHistoryStatus::Failed
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataTableView {
    Summary,
    Detail,
    Compare,
}

impl DataTableView {
    fn label(self) -> &'static str {
        match self {
            Self::Summary => "汇总表",
            Self::Detail => "明细表",
            Self::Compare => "对比表",
        }
    }
}

impl DataExtractPanel {
    pub fn new() -> Self {
        let mut remote_config = crate::remote::RemoteConfig::default();
        remote_config.apply_env_overrides();
        let data_source = crate::remote::DataSource::from_remote_enabled(
            crate::remote::remote_enabled(&remote_config),
        );
        let mut panel = Self {
            batches: Vec::new(),
            current_batch: None,
            baseline_batch: None,
            selected_record: None,
            module_filter: None,
            status_filter: None,
            search_buf: String::new(),
            threshold_profile: ThresholdService::load_or_default(),
            comparison: None,
            table_view: DataTableView::Summary,
            action_history: GuiPrefs::load().action_history,
            summary_cache: None,
            summary_dirty: true,
            export_column_keys: Vec::new(),
            export_columns_initialized: false,
            export_template_name: String::from("默认导出"),
            selected_export_cell: None,
            edit_cell_buf: String::new(),
            export_overrides: BTreeMap::new(),
            batch_rename_buf: String::new(),
            pending_delete_batch: None,
            error: None,
            status_hint: String::from("导入 Imatest 结果目录或文件开始提取"),
            output: DataExtractPanelOutput::default(),
            remote_config,
            data_source,
            remote_results: Vec::new(),
            results_fetch: None,
            report_fetch: None,
            remote_loading: false,
        };
        if panel.data_source == crate::remote::DataSource::Remote {
            panel.start_remote_results_fetch();
        }
        panel
    }

    pub fn take_output(&mut self) -> DataExtractPanelOutput {
        std::mem::take(&mut self.output)
    }

    pub fn set_remote_config(&mut self, remote_config: crate::remote::RemoteConfig) {
        if self.remote_config == remote_config {
            return;
        }
        self.remote_config = remote_config;
        let want = crate::remote::DataSource::from_remote_enabled(crate::remote::remote_enabled(
            &self.remote_config,
        ));
        if self.data_source != want {
            self.data_source = want;
            if want == crate::remote::DataSource::Remote {
                self.start_remote_results_fetch();
            } else {
                self.switch_to_local("远程未启用，已使用本地结果");
            }
        }
    }

    pub fn refresh_remote_catalog(&mut self) {
        self.data_source = crate::remote::DataSource::Remote;
        self.start_remote_results_fetch();
    }

    pub fn ui(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        self.poll_remote_fetches(ctx);
        if let Some(err) = self.error.take() {
            widgets::error_banner(ui, &err);
            ui.add_space(6.0);
        }

        widgets::navigation_header(ui, "导入指标、阈值判定与批次对比");
        widgets::page_header_gap(ui);
        self.filter_toolbar_ui(ui);
        widgets::section_gap(ui);
        self.main_body_ui(ui);
    }

    fn main_body_ui(&mut self, ui: &mut egui::Ui) {
        let geo = widgets::SideMainGeometry::compute(
            ui.available_size(),
            theme::DATA_EXTRACT_WIDE_BREAKPOINT,
            theme::DATA_EXTRACT_LEFT_W,
        );

        match geo.mode {
            widgets::SideMainMode::SideBySide => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.allocate_ui_with_layout(
                        egui::vec2(geo.left_w, geo.row_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(geo.left_w);
                            ui.set_max_width(geo.left_w);
                            ui.set_width(geo.left_w);
                            egui::ScrollArea::vertical()
                                .id_salt("de_left_sidebar")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let content_w =
                                        ui.available_width().min(ui.max_rect().width()).max(120.0);
                                    ui.set_width(content_w);
                                    self.left_sidebar_ui(ui);
                                });
                        },
                    );
                    ui.add_space(geo.gap);
                    ui.allocate_ui_with_layout(
                        egui::vec2(geo.main_w, geo.row_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_width(geo.main_w);
                            ui.set_max_width(geo.main_w);
                            ui.set_width(geo.main_w);
                            self.render_active_table(ui, geo.main_w);
                        },
                    );
                    ui.allocate_exact_size(
                        egui::vec2(geo.right_inset, geo.row_h),
                        egui::Sense::hover(),
                    );
                });
            }
            widgets::SideMainMode::Stacked => {
                let side_w = ui.available_width();
                egui::ScrollArea::vertical()
                    .id_salt("de_left_sidebar_narrow")
                    .max_height(geo.side_max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_width(side_w);
                        self.left_sidebar_ui(ui);
                    });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                let table_w = ui.available_width().max(160.0);
                self.render_active_table(ui, table_w);
            }
        }
    }

    fn render_active_table(&mut self, ui: &mut egui::Ui, table_w: f32) {
        match self.active_table_view() {
            DataTableView::Summary => self.summary_table_ui(ui, table_w),
            DataTableView::Detail => self.detail_table_ui(ui, table_w),
            DataTableView::Compare => self.comparison_table_ui(ui, table_w),
        }
    }

    fn active_table_view(&self) -> DataTableView {
        if self.table_view == DataTableView::Compare && self.comparison.is_none() {
            DataTableView::Summary
        } else {
            self.table_view
        }
    }

    fn filter_toolbar_ui(&mut self, ui: &mut egui::Ui) {
        let module_label = self
            .module_filter
            .map(|m| m.short_label().to_string())
            .unwrap_or_else(|| "全部".to_string());
        let has_batch = self.current_batch.is_some();
        let has_comparison = self.comparison.is_some();
        let has_export_rows = self.has_export_rows();
        let can_export_json = self.active_table_view() != DataTableView::Compare && has_export_rows;
        let narrow = ui.available_width() < theme::DATA_EXTRACT_WIDE_BREAKPOINT;

        // 视图切换：窄屏用等分分段条，宽屏用芯片行
        if narrow {
            widgets::mode_tab_bar(
                ui,
                &mut self.table_view,
                &[
                    (DataTableView::Summary, DataTableView::Summary.label()),
                    (DataTableView::Detail, DataTableView::Detail.label()),
                    (DataTableView::Compare, DataTableView::Compare.label()),
                ],
            );
            if self.table_view == DataTableView::Compare && !has_comparison {
                self.table_view = DataTableView::Summary;
            }
            ui.add_space(8.0);
        }

        widgets::toolbar_row(ui, |ui| {
            if !narrow {
                for view in [
                    DataTableView::Summary,
                    DataTableView::Detail,
                    DataTableView::Compare,
                ] {
                    let enabled = view != DataTableView::Compare || has_comparison;
                    if widgets::toggle_chip(
                        ui,
                        view.label(),
                        self.active_table_view() == view,
                        enabled,
                    ) {
                        self.table_view = view;
                    }
                }
                widgets::toolbar_separator(ui);
            }

            widgets::toolbar_field_label(ui, "模块", ui.style().visuals.dark_mode);
            let combo_w = if narrow { 88.0 } else { 100.0 };
            widgets::toolbar_combo_box(ui, "de_module_filter", &module_label, combo_w, |ui| {
                if ui
                    .selectable_label(self.module_filter.is_none(), "全部")
                    .clicked()
                {
                    self.module_filter = None;
                }
                for module in ImatestModule::ALL {
                    let selected = self.module_filter == Some(module);
                    if ui
                        .selectable_label(selected, module.short_label())
                        .clicked()
                    {
                        self.module_filter = Some(module);
                    }
                }
            });

            ui.add_space(4.0);
            widgets::toolbar_field_label(ui, "状态", ui.style().visuals.dark_mode);
            let all_status = self.status_filter.is_none();
            if widgets::toggle_chip(ui, "全部", all_status, true) {
                self.status_filter = None;
            }
            for st in [
                EvaluationStatus::Pass,
                EvaluationStatus::Warn,
                EvaluationStatus::Fail,
            ] {
                let selected = self.status_filter == Some(st);
                if widgets::toggle_chip(ui, st.label(), selected, true) {
                    self.status_filter = Some(st);
                }
            }
        });

        ui.add_space(6.0);

        widgets::toolbar_row(ui, |ui| {
            let search_w = if narrow {
                (ui.available_width() - 8.0).max(120.0)
            } else {
                (ui.available_width() - 360.0).max(120.0)
            };
            widgets::toolbar_search_edit(
                ui,
                &mut self.search_buf,
                "搜索指标…",
                search_w.min(ui.available_width()),
            );

            if widgets::compact_secondary_button(ui, "重新解析", has_batch).clicked() {
                self.reparse_current();
            }
            if widgets::compact_primary_button(ui, "导出 CSV", has_export_rows).clicked() {
                self.export_csv();
            }
            if widgets::compact_secondary_button(ui, "导出 JSON", can_export_json).clicked() {
                self.export_json();
            }
            if widgets::compact_secondary_button(ui, "导出报告", has_export_rows).clicked() {
                self.export_html_report();
            }
        });
    }

    fn left_sidebar_ui(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        widgets::grouped_section(ui, "导入", |ui| {
            if widgets::full_width_primary_button(ui, "导入目录…", true).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.import_path(path);
                }
            }
            ui.add_space(6.0);
            if widgets::full_width_secondary_button(ui, "导入文件…", true).clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.import_path(path);
                }
            }
            ui.add_space(6.0);
            if widgets::full_width_secondary_button(ui, "批量导入文件…", true).clicked() {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    self.import_paths(paths);
                }
            }
            ui.add_space(6.0);
            let half = ((ui.available_width() - 6.0) * 0.5).max(72.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.vertical(|ui| {
                    ui.set_width(half);
                    if widgets::full_width_secondary_button(
                        ui,
                        "保存草稿",
                        !self.batches.is_empty(),
                    )
                    .clicked()
                    {
                        self.save_project_draft();
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(half);
                    if widgets::full_width_secondary_button(ui, "恢复草稿…", true).clicked() {
                        if let Some(root) = rfd::FileDialog::new().pick_folder() {
                            self.load_project_draft(root);
                        }
                    }
                });
            });
            #[cfg(feature = "ocr")]
            {
                ui.add_space(6.0);
                let avail = check_availability();
                let ocr_ok = avail.tesseract_ok;
                if widgets::full_width_secondary_button(ui, "导入截图 OCR…", ocr_ok).clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("图片", &["png", "jpg", "jpeg", "tiff", "bmp", "webp"])
                        .pick_file()
                    {
                        self.import_path(path);
                    }
                }
                if !ocr_ok {
                    ui.label(RichText::new("OCR 未就绪").small().weak())
                        .on_hover_text(&avail.detail);
                }
            }
        });

        ui.add_space(8.0);
        widgets::grouped_section(ui, "阈值", |ui| {
            ui.label(format!("规则数：{}", self.threshold_profile.rules.len()));
            ui.add_space(4.0);
            let gap = 6.0;
            let cell = ((ui.available_width() - gap) * 0.5).max(64.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.vertical(|ui| {
                    ui.set_width(cell);
                    if widgets::full_width_secondary_button(ui, "加载", true).clicked() {
                        self.load_thresholds();
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(cell);
                    if widgets::full_width_secondary_button(ui, "保存", true).clicked() {
                        self.save_thresholds();
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.vertical(|ui| {
                    ui.set_width(cell);
                    if widgets::full_width_secondary_button(ui, "默认", true).clicked() {
                        self.threshold_profile = ThresholdProfile::default_rules();
                        self.reapply_thresholds();
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(cell);
                    if widgets::full_width_secondary_button(
                        ui,
                        "全部重算",
                        !self.batches.is_empty(),
                    )
                    .clicked()
                    {
                        self.reapply_thresholds_all();
                    }
                });
            });
        });

        ui.add_space(8.0);
        widgets::grouped_section(ui, "批次", |ui| {
            self.remote_results_source_ui(ui);
            if crate::remote::remote_enabled(&self.remote_config) {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            if self.batches.is_empty() {
                ui.label("暂无批次");
            } else {
                ScrollArea::vertical()
                    .id_salt("data_extract_batches")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for (idx, batch) in self.batches.clone().iter().enumerate() {
                            let selected = self.current_batch == Some(idx);
                            let is_baseline = self.baseline_batch == Some(idx);
                            let prefix = if is_baseline { "★ " } else { "" };
                            let label = format!("{prefix}{}\n{}", batch.name, batch.summary_line());
                            if ui.selectable_label(selected, label).clicked() {
                                self.current_batch = Some(idx);
                                self.selected_record = None;
                                self.batch_rename_buf = batch.name.clone();
                            }
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let has_current = self.current_batch.is_some();
                    if widgets::compact_secondary_button(ui, "设为基准", has_current).clicked()
                    {
                        self.baseline_batch = self.current_batch;
                        self.comparison = None;
                        self.table_view = DataTableView::Summary;
                    }
                    let can_compare = self.baseline_batch.is_some()
                        && self.current_batch.is_some()
                        && self.baseline_batch != self.current_batch;
                    if widgets::compact_primary_button(ui, "对比", can_compare).clicked() {
                        self.run_compare();
                    }
                });
                ui.add_space(4.0);
                let has_current = self.current_batch.is_some();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(has_current, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.batch_rename_buf)
                                .hint_text("批次名称")
                                .desired_width(130.0),
                        );
                    });
                    if widgets::compact_secondary_button(ui, "重命名", has_current).clicked() {
                        self.rename_current_batch();
                    }
                    if widgets::compact_secondary_button(ui, "删除", has_current).clicked() {
                        self.pending_delete_batch = self.current_batch;
                    }
                });
                if let Some(idx) = self.pending_delete_batch {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("确认删除当前批次？");
                        if widgets::compact_primary_button(ui, "确认", true).clicked() {
                            self.delete_batch(idx);
                            self.pending_delete_batch = None;
                        }
                        if widgets::compact_secondary_button(ui, "取消", true).clicked() {
                            self.pending_delete_batch = None;
                        }
                    });
                }
                if let Some(bi) = self.baseline_batch {
                    if let Some(b) = self.batches.get(bi) {
                        ui.label(RichText::new(format!("基准：{}", b.name)).small().weak());
                    }
                }
            }
        });

        ui.add_space(8.0);
        self.insights_ui(ui);

        ui.add_space(8.0);
        self.recent_tasks_ui(ui);

        ui.add_space(8.0);
        // 详情跟侧栏一起滚：勿再套固定 max_height ScrollArea，否则底边常被视口裁切
        self.detail_ui(ui);
        // 底部安全区：滚到底时整块「详情」仍完整可见
        ui.add_space(28.0);
    }

    fn summary_table_ui(&mut self, ui: &mut egui::Ui, width: f32) {
        let table = self.summary_table();
        fixed_grouped_section(ui, "汇总表", width, |ui| {
            if table.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("无汇总结果，请导入 Imatest 结果文件");
                });
                return;
            }

            self.export_controls_ui(ui, &table);
            ui.add_space(4.0);
            self.summary_grid_ui(ui, &table);
        });
    }

    fn summary_grid_ui(&mut self, ui: &mut egui::Ui, table: &SummaryTable) {
        let search = self.search_buf.to_ascii_lowercase();
        let query = DataQueryService::parse(&self.search_buf);
        let insights = self.current_insights(table);
        let outlier_metrics: Vec<String> = insights
            .outliers
            .iter()
            .map(|outlier| outlier.metric.clone())
            .collect();
        let rows: Vec<_> = table
            .rows
            .iter()
            .filter(|row| {
                self.summary_row_matches(row, &search)
                    && DataQueryService::matches_row(&query, row, &outlier_metrics)
            })
            .collect();

        if rows.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("无匹配汇总结果");
            });
            return;
        }

        ScrollArea::both()
            .id_salt("data_extract_summary_table")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("data_extract_summary_grid")
                    .num_columns(7 + table.columns.len())
                    .spacing([10.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("状态").strong());
                        ui.label(RichText::new("批次").strong());
                        ui.label(RichText::new("样本").strong());
                        ui.label(RichText::new("来源").strong());
                        ui.label(RichText::new("来源类型").strong());
                        ui.label(RichText::new("警告").strong());
                        ui.label(RichText::new("冲突").strong());
                        for col in &table.columns {
                            ui.label(RichText::new(&col.label).strong());
                        }
                        ui.end_row();

                        for row in rows {
                            let selected = self.current_batch == Some(row.batch_index);
                            let status = RichText::new(row.status.label())
                                .color(status_color(row.status, ui.visuals().dark_mode));
                            if ui.selectable_label(selected, status).clicked() {
                                self.select_summary_row(row);
                            }
                            if ui.selectable_label(selected, &row.batch_name).clicked() {
                                self.select_summary_row(row);
                            }
                            if ui.selectable_label(selected, &row.sample_name).clicked() {
                                self.select_summary_row(row);
                            }
                            let source = row
                                .source_path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| row.source_path.display().to_string());
                            if ui.selectable_label(selected, source).clicked() {
                                self.select_summary_row(row);
                            }
                            if ui
                                .selectable_label(selected, row.source_kind.label())
                                .clicked()
                            {
                                self.select_summary_row(row);
                            }
                            if ui
                                .selectable_label(selected, row.warning_count.to_string())
                                .clicked()
                            {
                                self.select_summary_row(row);
                            }
                            if ui
                                .selectable_label(selected, row.conflict_count.to_string())
                                .clicked()
                            {
                                self.select_summary_row(row);
                            }

                            for col in &table.columns {
                                if let Some(cell) = row.values.get(&col.key) {
                                    let text = RichText::new(&cell.display)
                                        .color(status_color(cell.status, ui.visuals().dark_mode));
                                    if ui.selectable_label(selected, text).clicked() {
                                        self.current_batch = Some(cell.record_ref.batch_index);
                                        self.selected_record = Some(cell.record_ref.record_index);
                                        self.select_export_cell(row, &col.key, &cell.display);
                                    }
                                } else {
                                    ui.label("—");
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn summary_row_matches(
        &self,
        row: &crate::data_extract::domain::SummaryRow,
        search: &str,
    ) -> bool {
        if let Some(st) = self.status_filter {
            if row.status != st {
                return false;
            }
        }

        if search.is_empty() {
            return true;
        }

        row.batch_name.to_ascii_lowercase().contains(search)
            || row.sample_name.to_ascii_lowercase().contains(search)
            || row
                .source_path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains(search)
            || row.values.iter().any(|(key, cell)| {
                key.to_ascii_lowercase().contains(search)
                    || cell.display.to_ascii_lowercase().contains(search)
            })
    }

    fn select_summary_row(&mut self, row: &crate::data_extract::domain::SummaryRow) {
        self.current_batch = Some(row.batch_index);
        self.selected_record = row
            .values
            .values()
            .next()
            .map(|cell| cell.record_ref.record_index);
    }

    fn export_controls_ui(&mut self, ui: &mut egui::Ui, table: &SummaryTable) {
        let schema = self.schema_for_table(table);
        let enabled_count = schema.columns.iter().filter(|c| c.enabled).count();
        ui.label(
            RichText::new(format!(
                "导出预览：{} 行 · {} / {} 列 · 覆盖 {} 个单元格",
                table.row_count(),
                enabled_count,
                schema.columns.len(),
                self.export_overrides.len()
            ))
            .weak()
            .size(11.0),
        );
        ui.horizontal_wrapped(|ui| {
            if widgets::compact_secondary_button(ui, "全选列", true).clicked() {
                self.export_column_keys = schema.columns.iter().map(|c| c.key.clone()).collect();
                self.export_columns_initialized = true;
            }
            if widgets::compact_secondary_button(ui, "清空列", true).clicked() {
                self.export_column_keys.clear();
                self.export_columns_initialized = true;
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.export_template_name)
                    .hint_text("模板名")
                    .desired_width(110.0),
            );
            if widgets::compact_primary_button(ui, "保存模板", enabled_count > 0).clicked() {
                self.save_export_template("数据提取");
            }
        });
        let templates = GuiPrefs::load().export_templates_for("数据提取");
        if !templates.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("模板").small().weak());
                for template in templates.iter().take(4) {
                    if widgets::compact_secondary_button(ui, &template.name, true).clicked() {
                        self.export_column_keys = template.columns.clone();
                        self.export_columns_initialized = true;
                        self.export_template_name = template.name.clone();
                    }
                }
            });
        }
        ui.collapsing("导出列", |ui| {
            ui.horizontal_wrapped(|ui| {
                for column in schema.columns.clone() {
                    let mut on = self.export_column_keys.contains(&column.key);
                    if ui.checkbox(&mut on, &column.label).changed() {
                        self.export_columns_initialized = true;
                        if on {
                            if !self.export_column_keys.contains(&column.key) {
                                self.export_column_keys.push(column.key);
                            }
                        } else {
                            self.export_column_keys.retain(|key| key != &column.key);
                        }
                    }
                }
            });
        });
        if let Some(cell) = self.selected_export_cell.clone() {
            ui.separator();
            ui.label(RichText::new(format!("导出覆盖：{}", cell.label)).strong());
            ui.text_edit_singleline(&mut self.edit_cell_buf);
            ui.horizontal(|ui| {
                if widgets::compact_primary_button(ui, "应用覆盖", true).clicked() {
                    self.export_overrides
                        .insert(cell.storage_key(), self.edit_cell_buf.clone());
                }
                if widgets::compact_secondary_button(ui, "清除覆盖", true).clicked() {
                    self.export_overrides.remove(&cell.storage_key());
                }
            });
        }
    }

    fn schema_for_table(&mut self, table: &SummaryTable) -> TableExportSchema {
        let base = TableExportSchema::from_summary_table(table);
        let all_keys: Vec<String> = base.columns.iter().map(|c| c.key.clone()).collect();
        if !self.export_columns_initialized {
            self.export_column_keys = all_keys.clone();
            self.export_columns_initialized = true;
        } else {
            self.export_column_keys.retain(|key| all_keys.contains(key));
        }
        base.with_enabled_keys(&self.export_column_keys)
    }

    fn save_export_template(&mut self, module: &str) {
        let name = if self.export_template_name.trim().is_empty() {
            "默认导出".to_string()
        } else {
            self.export_template_name.trim().to_string()
        };
        let mut prefs = GuiPrefs::load();
        prefs.upsert_export_template(ExportTemplate {
            module: module.into(),
            name: name.clone(),
            columns: self.export_column_keys.clone(),
        });
        let _ = prefs.save();
        self.export_template_name = name.clone();
        self.status_hint = format!("已保存导出模板「{name}」");
    }

    fn select_export_cell(
        &mut self,
        row: &crate::data_extract::domain::SummaryRow,
        column_key: &str,
        current_value: &str,
    ) {
        let key = EditableCellKey {
            row_key: summary_row_storage_key(row),
            column_key: column_key.to_string(),
            label: format!("{} / {} / {}", row.batch_name, row.sample_name, column_key),
        };
        self.edit_cell_buf = self
            .export_overrides
            .get(&key.storage_key())
            .cloned()
            .unwrap_or_else(|| current_value.to_string());
        self.selected_export_cell = Some(key);
    }

    fn table_with_overrides(&self, table: &SummaryTable) -> SummaryTable {
        let mut table = table.clone();
        for row in &mut table.rows {
            let row_key = summary_row_storage_key(row);
            for (column_key, cell) in &mut row.values {
                let key = format!("{row_key}::{column_key}");
                if let Some(value) = self.export_overrides.get(&key) {
                    cell.display = value.clone();
                }
            }
        }
        table
    }

    fn detail_table_ui(&mut self, ui: &mut egui::Ui, width: f32) {
        let export_table = SummaryService::detail_table(&self.batches);
        fixed_grouped_section(ui, "明细表", width, |ui| {
            if !export_table.is_empty() {
                self.export_controls_ui(ui, &export_table);
                ui.add_space(4.0);
            }
            let records = self.filtered_detail_records();
            if records.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("无匹配结果，请导入 Imatest 结果文件");
                });
                return;
            }

            let table_w = ui.available_width();
            ScrollArea::vertical()
                .id_salt("data_extract_table")
                .max_height(ui.available_height().max(200.0))
                .show(ui, |ui| {
                    ui.set_width(table_w);
                    egui::Grid::new("data_extract_grid")
                        .num_columns(10)
                        .spacing([6.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("状态").strong());
                            ui.label(RichText::new("批次").strong());
                            ui.label(RichText::new("模块").strong());
                            ui.label(RichText::new("指标").strong());
                            ui.label(RichText::new("值").strong());
                            ui.label(RichText::new("单位").strong());
                            ui.label(RichText::new("样本").strong());
                            ui.label(RichText::new("来源类型").strong());
                            ui.label(RichText::new("文件").strong());
                            ui.label(RichText::new("警告").strong());
                            ui.end_row();

                            for (batch_idx, record_idx, batch_name, rec) in records.iter() {
                                let selected = self.current_batch == Some(*batch_idx)
                                    && self.selected_record == Some(*record_idx);
                                let st = rec.evaluation_status();
                                let st_label = RichText::new(st.label())
                                    .color(status_color(st, ui.visuals().dark_mode));
                                if ui.selectable_label(selected, st_label).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui.selectable_label(selected, batch_name).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui
                                    .selectable_label(selected, rec.module.short_label())
                                    .clicked()
                                {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui.selectable_label(selected, &rec.metric_key).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui
                                    .selectable_label(selected, rec.value.display_value())
                                    .clicked()
                                {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                let unit = rec.value.unit.as_deref().unwrap_or("—");
                                if ui.selectable_label(selected, unit).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui
                                    .selectable_label(
                                        selected,
                                        rec.sample_name.as_deref().unwrap_or("—"),
                                    )
                                    .clicked()
                                {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                if ui
                                    .selectable_label(selected, rec.source_kind.label())
                                    .clicked()
                                {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                let src = rec
                                    .source_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| rec.source_path.display().to_string());
                                if ui.selectable_label(selected, src).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                let warn = if rec.warnings.is_empty() {
                                    "—".to_string()
                                } else {
                                    rec.warnings.len().to_string()
                                };
                                if ui.selectable_label(selected, warn).clicked() {
                                    self.current_batch = Some(*batch_idx);
                                    self.selected_record = Some(*record_idx);
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn comparison_table_ui(&mut self, ui: &mut egui::Ui, width: f32) {
        let Some(cmp) = self.comparison.clone() else {
            self.table_view = DataTableView::Summary;
            return;
        };
        fixed_grouped_section(ui, "批次对比", width, |ui| {
            ui.label(format!(
                "基准「{}」 vs 当前「{}」",
                cmp.baseline_batch_name, cmp.current_batch_name
            ));
            ui.horizontal(|ui| {
                if widgets::compact_secondary_button(ui, "返回记录", true).clicked() {
                    self.table_view = DataTableView::Summary;
                }
                if widgets::compact_primary_button(ui, "导出对比 CSV", !cmp.rows.is_empty())
                    .clicked()
                {
                    self.export_comparison();
                }
            });
            ui.add_space(6.0);
            let table_w = ui.available_width();
            ScrollArea::vertical()
                .id_salt("data_extract_compare")
                .max_height(ui.available_height().max(200.0))
                .show(ui, |ui| {
                    ui.set_width(table_w);
                    egui::Grid::new("data_extract_compare_grid")
                        .num_columns(8)
                        .spacing([6.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label(RichText::new("模块").strong());
                            ui.label(RichText::new("指标").strong());
                            ui.label(RichText::new("基准").strong());
                            ui.label(RichText::new("当前").strong());
                            ui.label(RichText::new("差值").strong());
                            ui.label(RichText::new("变化%").strong());
                            ui.label(RichText::new("趋势").strong());
                            ui.label(RichText::new("状态").strong());
                            ui.end_row();
                            for row in &cmp.rows {
                                ui.label(row.module.short_label());
                                ui.label(&row.metric_key);
                                ui.label(fmt_opt(row.baseline_value));
                                ui.label(fmt_opt(row.current_value));
                                ui.label(fmt_opt(row.delta));
                                ui.label(
                                    row.delta_pct
                                        .map(|p| format!("{p:.1}%"))
                                        .unwrap_or_else(|| "—".into()),
                                );
                                let dark = ui.visuals().dark_mode;
                                let trend_color = match row.trend {
                                    crate::data_extract::domain::TrendStatus::Improved => {
                                        theme::success_color(dark)
                                    }
                                    crate::data_extract::domain::TrendStatus::Regressed => {
                                        theme::error_color(dark)
                                    }
                                    _ => theme::secondary_label(dark),
                                };
                                ui.label(RichText::new(row.trend.label()).color(trend_color));
                                ui.label(format!(
                                    "{} → {}",
                                    row.baseline_status.label(),
                                    row.current_status.label()
                                ));
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn detail_ui(&mut self, ui: &mut egui::Ui) {
        widgets::grouped_section(ui, "详情", |ui| {
            let Some(rec) = self.selected_record().cloned() else {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("选择一条记录查看详情")
                        .color(theme::secondary_label(ui.visuals().dark_mode)),
                );
                ui.add_space(4.0);
                if let Some(batch) = self.current_batch() {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.label(RichText::new("批次汇总").strong());
                    ui.label(format!("来源：{}", batch.source_root.display()));
                    ui.label(batch.summary_line());
                    if let Some(ref es) = batch.evaluation_summary {
                        ui.label(format!(
                            "评价：通过 {} / 警告 {} / 失败 {} / 未判定 {}",
                            es.pass, es.warn, es.fail, es.unknown
                        ));
                    }
                    if !batch.unmapped_fields.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("未映射字段")
                                .color(theme::warning_color(ui.visuals().dark_mode)),
                        );
                        ScrollArea::vertical()
                            .id_salt("unmapped_fields")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for uf in batch.unmapped_fields.iter().take(20) {
                                    ui.label(format!(
                                        "• {} / {}（{} 次）",
                                        uf.module.short_label(),
                                        uf.raw_name,
                                        uf.count
                                    ));
                                }
                            });
                    }
                    if !batch.warnings.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("警告")
                                .color(theme::warning_color(ui.visuals().dark_mode)),
                        );
                        for w in batch.warnings.iter().take(10) {
                            ui.label(format!("• {}", w.message));
                        }
                    }
                }
                return;
            };

            let st = rec.evaluation_status();
            ui.label(
                RichText::new(format!("状态：{}", st.label()))
                    .color(status_color(st, ui.visuals().dark_mode)),
            );
            ui.label(format!("模块：{}", rec.module.label()));
            ui.label(format!("指标键：{}", rec.metric_key));
            ui.label(format!("原始字段：{}", rec.raw_name));
            ui.label(format!("值：{}", rec.value.display_value()));
            if let Some(ref u) = rec.value.unit {
                ui.label(format!("单位：{u}"));
            }
            if let Some(ref s) = rec.sample_name {
                ui.label(format!("样本：{s}"));
            }
            ui.label(format!("来源类型：{}", rec.source_kind.label()));
            ui.label(format!("解析器：{}", rec.parser_name));
            ui.label(format!("来源：{}", rec.source_path.display()));

            if let Some(ref ev) = rec.evaluation {
                ui.add_space(6.0);
                ui.label(RichText::new("阈值评价").strong());
                if let Some(ref desc) = ev.rule_description {
                    ui.label(format!("规则：{desc}"));
                }
                if let Some(ref reason) = ev.reason {
                    ui.label(format!("说明：{reason}"));
                }
            }

            if let Some(ref ocr) = rec.ocr {
                ui.add_space(6.0);
                ui.label(RichText::new("OCR").strong());
                ui.label(format!("引擎：{}", ocr.engine));
                if let Some(c) = ocr.confidence {
                    let dark = ui.visuals().dark_mode;
                    let color = if c < 60.0 {
                        theme::warning_color(dark)
                    } else {
                        theme::secondary_label(dark)
                    };
                    ui.label(RichText::new(format!("置信度：{c:.1}%")).color(color));
                }
                if let Some(ref cache) = ocr.text_cache_path {
                    if let Ok(text) = std::fs::read_to_string(cache) {
                        ui.add_space(4.0);
                        ui.label("识别文本：");
                        ScrollArea::vertical()
                            .id_salt("ocr_text_view")
                            .max_height(160.0)
                            .show(ui, |ui| {
                                ui.label(text);
                            });
                    }
                }
            }

            if !rec.warnings.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("记录警告").color(theme::warning_color(ui.visuals().dark_mode)),
                );
                for w in &rec.warnings {
                    ui.label(format!("• {}", w.message));
                }
            }
        });
    }

    fn import_path(&mut self, path: PathBuf) {
        let started = Instant::now();
        match DataExtractService::extract_with_thresholds(&path, Some(&self.threshold_profile)) {
            Ok(batch) => {
                let batch_name = batch.name.clone();
                self.status_hint = format!("已导入：{}", batch.summary_line());
                let total = batch.records.len();
                self.batch_rename_buf = batch.name.clone();
                self.batches.insert(0, batch);
                self.mark_summary_dirty();
                self.current_batch = Some(0);
                self.selected_record = None;
                self.comparison = None;
                self.table_view = DataTableView::Summary;
                if let Some(note) = self.submit_remote_data_extract(&[path.clone()], &batch_name) {
                    self.status_hint.push_str(&format!(" · {note}"));
                }
                self.output.status_message = self.status_hint.clone();
                self.record_action(
                    "导入",
                    path.display().to_string(),
                    ActionHistoryStatus::Succeeded,
                    total,
                    0,
                    total,
                    started.elapsed().as_millis() as u64,
                    None,
                );
            }
            Err(e) => {
                let msg = e.to_string();
                self.record_action(
                    "导入",
                    path.display().to_string(),
                    ActionHistoryStatus::Failed,
                    0,
                    1,
                    1,
                    started.elapsed().as_millis() as u64,
                    Some(msg.clone()),
                );
                self.error = Some(msg);
            }
        }
    }

    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let started = Instant::now();
        let mut result = BatchActionResult {
            total: paths.len(),
            ..Default::default()
        };
        for path in &paths {
            match DataExtractService::extract_with_thresholds(path, Some(&self.threshold_profile)) {
                Ok(batch) => {
                    self.batches.insert(0, batch);
                    result.successes += 1;
                }
                Err(e) => result.failures.push(format!("{}: {}", path.display(), e)),
            }
        }
        if result.successes > 0 {
            self.current_batch = Some(0);
            self.batch_rename_buf = self
                .batches
                .first()
                .map(|batch| batch.name.clone())
                .unwrap_or_default();
            self.selected_record = None;
            self.comparison = None;
            self.table_view = DataTableView::Summary;
            self.mark_summary_dirty();
        }
        self.status_hint = format!(
            "批量导入完成：成功 {} / 失败 {}",
            result.successes,
            result.failures.len()
        );
        if result.successes > 0 {
            if let Some(note) = self.submit_remote_data_extract(&paths, "批量数据提取") {
                self.status_hint.push_str(&format!(" · {note}"));
            }
        }
        self.output.status_message = self.status_hint.clone();
        if !result.failures.is_empty() {
            self.error = Some(
                result
                    .failures
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        self.record_action(
            "批量导入",
            "多个文件",
            result.status(),
            result.successes,
            result.failures.len(),
            result.total,
            started.elapsed().as_millis() as u64,
            (!result.failures.is_empty()).then(|| result.failures.join("\n")),
        );
    }

    fn load_thresholds(&mut self) {
        match ThresholdService::load() {
            Ok(p) => {
                self.threshold_profile = p;
                self.reapply_thresholds();
                self.status_hint = "已加载阈值配置".into();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn save_thresholds(&mut self) {
        match ThresholdService::save(&self.threshold_profile) {
            Ok(()) => self.status_hint = "已保存阈值配置".into(),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn reapply_thresholds(&mut self) {
        let started = Instant::now();
        if let Some(idx) = self.current_batch {
            if let Some(batch) = self.batches.get_mut(idx) {
                batch.apply_thresholds(&self.threshold_profile.clone());
                let total = batch.records.len();
                let target = batch.name.clone();
                self.mark_summary_dirty();
                self.record_action(
                    "阈值重算",
                    target,
                    ActionHistoryStatus::Succeeded,
                    total,
                    0,
                    total,
                    started.elapsed().as_millis() as u64,
                    None,
                );
            }
        }
    }

    fn reapply_thresholds_all(&mut self) {
        let started = Instant::now();
        let mut total = 0usize;
        for batch in &mut self.batches {
            batch.apply_thresholds(&self.threshold_profile.clone());
            total += batch.records.len();
        }
        self.mark_summary_dirty();
        self.status_hint = format!("已为 {} 个批次重算阈值", self.batches.len());
        self.output.status_message = self.status_hint.clone();
        self.record_action(
            "批量阈值重算",
            format!("{} 个批次", self.batches.len()),
            ActionHistoryStatus::Succeeded,
            total,
            0,
            total,
            started.elapsed().as_millis() as u64,
            None,
        );
    }

    fn reparse_current(&mut self) {
        let Some(batch) = self.current_batch().cloned() else {
            return;
        };
        let started = Instant::now();
        match DataExtractService::extract_with_thresholds(
            &batch.source_root,
            Some(&self.threshold_profile),
        ) {
            Ok(new_batch) => {
                if let Some(idx) = self.current_batch {
                    let total = new_batch.records.len();
                    let batch_name = new_batch.name.clone();
                    self.batch_rename_buf = new_batch.name.clone();
                    self.batches[idx] = new_batch;
                    self.mark_summary_dirty();
                    self.status_hint = format!("已重新解析：{}", self.batches[idx].summary_line());
                    if let Some(note) =
                        self.submit_remote_data_extract(&[batch.source_root.clone()], &batch_name)
                    {
                        self.status_hint.push_str(&format!(" · {note}"));
                    }
                    self.record_action(
                        "重新解析",
                        batch.source_root.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        total,
                        0,
                        total,
                        started.elapsed().as_millis() as u64,
                        None,
                    );
                }
            }
            Err(e) => {
                let msg = e.to_string();
                self.record_action(
                    "重新解析",
                    batch.source_root.display().to_string(),
                    ActionHistoryStatus::Failed,
                    0,
                    1,
                    1,
                    started.elapsed().as_millis() as u64,
                    Some(msg.clone()),
                );
                self.error = Some(msg);
            }
        }
    }

    fn rename_current_batch(&mut self) {
        let Some(idx) = self.current_batch else {
            return;
        };
        let new_name = self.batch_rename_buf.trim().to_string();
        if new_name.is_empty() {
            self.error = Some("批次名称不能为空".into());
            return;
        }
        let Some(batch) = self.batches.get_mut(idx) else {
            return;
        };
        let old_name = batch.name.clone();
        if old_name == new_name {
            return;
        }
        batch.name = new_name.clone();
        self.mark_summary_dirty();
        self.status_hint = format!("已重命名批次：{old_name} → {new_name}");
        self.output.status_message = self.status_hint.clone();
        self.record_action(
            "重命名批次",
            format!("{old_name} → {new_name}"),
            ActionHistoryStatus::Succeeded,
            1,
            0,
            1,
            0,
            None,
        );
    }

    fn delete_batch(&mut self, idx: usize) {
        if idx >= self.batches.len() {
            return;
        }
        let removed = self.batches.remove(idx);
        self.selected_record = None;
        self.comparison = None;
        self.mark_summary_dirty();
        self.current_batch = if self.batches.is_empty() {
            None
        } else {
            Some(idx.min(self.batches.len() - 1))
        };
        self.batch_rename_buf = self
            .current_batch
            .and_then(|idx| self.batches.get(idx))
            .map(|batch| batch.name.clone())
            .unwrap_or_default();
        self.baseline_batch = self.baseline_batch.and_then(|baseline| {
            if baseline == idx {
                None
            } else if baseline > idx {
                Some(baseline - 1)
            } else {
                Some(baseline)
            }
        });
        self.status_hint = format!("已删除批次：{}", removed.name);
        self.output.status_message = self.status_hint.clone();
        self.record_action(
            "删除批次",
            removed.name,
            ActionHistoryStatus::Succeeded,
            removed.records.len(),
            0,
            removed.records.len(),
            0,
            None,
        );
    }

    fn save_project_draft(&mut self) {
        let Some(root) = self.current_batch().map(|b| b.source_root.clone()) else {
            return;
        };
        let draft = DataExtractDraft {
            batches: self.batches.clone(),
            current_batch: self.current_batch,
            baseline_batch: self.baseline_batch,
            export_column_keys: self.export_column_keys.clone(),
            export_overrides: self.export_overrides.clone(),
        };
        let path = root.join(".imgforge").join("data_extract_draft.json");
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, serde_json::to_string_pretty(&draft)?)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.status_hint = format!("已保存草稿：{}", path.display());
                self.output.status_message = self.status_hint.clone();
                self.record_action(
                    "保存草稿",
                    path.display().to_string(),
                    ActionHistoryStatus::Succeeded,
                    self.batches.len(),
                    0,
                    self.batches.len(),
                    0,
                    None,
                );
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn load_project_draft(&mut self, root: PathBuf) {
        let path = root.join(".imgforge").join("data_extract_draft.json");
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<DataExtractDraft>(&raw).ok())
        {
            Some(draft) => {
                self.batches = draft.batches;
                self.current_batch = draft.current_batch.filter(|idx| *idx < self.batches.len());
                self.baseline_batch = draft.baseline_batch.filter(|idx| *idx < self.batches.len());
                self.export_column_keys = draft.export_column_keys;
                self.export_columns_initialized = true;
                self.export_overrides = draft.export_overrides;
                self.selected_record = None;
                self.comparison = None;
                self.mark_summary_dirty();
                self.batch_rename_buf = self
                    .current_batch
                    .and_then(|idx| self.batches.get(idx))
                    .map(|batch| batch.name.clone())
                    .unwrap_or_default();
                self.status_hint = format!("已恢复草稿：{}", path.display());
                self.output.status_message = self.status_hint.clone();
                self.record_action(
                    "恢复草稿",
                    path.display().to_string(),
                    ActionHistoryStatus::Succeeded,
                    self.batches.len(),
                    0,
                    self.batches.len(),
                    0,
                    None,
                );
            }
            None => {
                self.error = Some(format!("未找到可恢复草稿：{}", path.display()));
            }
        }
    }

    fn run_compare(&mut self) {
        let baseline_idx = match self.baseline_batch {
            Some(i) => i,
            None => return,
        };
        let current_idx = match self.current_batch {
            Some(i) => i,
            None => return,
        };
        if baseline_idx == current_idx {
            return;
        }
        let baseline = self.batches[baseline_idx].clone();
        let current = self.batches[current_idx].clone();
        self.comparison = Some(CompareService::compare(&baseline, &current));
        self.table_view = DataTableView::Compare;
        self.status_hint = format!(
            "已对比 {} 条指标",
            self.comparison.as_ref().map(|c| c.rows.len()).unwrap_or(0)
        );
    }

    fn export_csv(&mut self) {
        let view = self.active_table_view();
        let file_name = match view {
            DataTableView::Summary => "imatest_summary.csv",
            DataTableView::Detail => "imatest_detail.csv",
            DataTableView::Compare => "imatest_compare.csv",
        };
        if let Some(dest) = rfd::FileDialog::new().set_file_name(file_name).save_file() {
            let started = Instant::now();
            let result = match view {
                DataTableView::Summary => {
                    let raw_table = self.summary_table();
                    let table = self.table_with_overrides(&raw_table);
                    let schema = self.schema_for_table(&table);
                    DataExportService::export_summary_csv_with_schema(&table, &schema, &dest)
                }
                DataTableView::Detail => {
                    let raw_table = SummaryService::detail_table(&self.batches);
                    let table = self.table_with_overrides(&raw_table);
                    let schema = self.schema_for_table(&table);
                    DataExportService::export_summary_csv_with_schema(&table, &schema, &dest)
                }
                DataTableView::Compare => {
                    let Some(cmp) = self.comparison.clone() else {
                        return;
                    };
                    DataExportService::export_comparison(&cmp, &dest)
                }
            };

            match result {
                Ok(r) => {
                    self.status_hint =
                        format!("已导出 CSV：{} 行 → {}", r.row_count, r.dest.display());
                    self.output.status_message = self.status_hint.clone();
                    self.record_action(
                        "导出 CSV",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.row_count,
                        0,
                        r.row_count,
                        started.elapsed().as_millis() as u64,
                        Some(view.label().into()),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出 CSV",
                        dest.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        1,
                        1,
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    self.error = Some(msg);
                }
            }
        }
    }

    fn export_json(&mut self) {
        let view = self.active_table_view();
        let file_name = match view {
            DataTableView::Summary => "imatest_summary.json",
            DataTableView::Detail => "imatest_extract.json",
            DataTableView::Compare => return,
        };
        if let Some(dest) = rfd::FileDialog::new().set_file_name(file_name).save_file() {
            let started = Instant::now();
            let result = match view {
                DataTableView::Summary => {
                    let raw_table = self.summary_table();
                    let table = self.table_with_overrides(&raw_table);
                    let schema = self.schema_for_table(&table);
                    let insights = self.current_insights(&table);
                    DataExportService::export_summary_json_with_insights(
                        &table,
                        &schema,
                        Some(&insights),
                        &dest,
                    )
                }
                DataTableView::Detail => {
                    let raw_table = SummaryService::detail_table(&self.batches);
                    let table = self.table_with_overrides(&raw_table);
                    let schema = self.schema_for_table(&table);
                    let insights = self.current_insights(&table);
                    DataExportService::export_summary_json_with_insights(
                        &table,
                        &schema,
                        Some(&insights),
                        &dest,
                    )
                }
                DataTableView::Compare => return,
            };

            match result {
                Ok(r) => {
                    self.status_hint =
                        format!("已导出 JSON：{} 行 → {}", r.row_count, r.dest.display());
                    self.output.status_message = self.status_hint.clone();
                    self.record_action(
                        "导出 JSON",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.row_count,
                        0,
                        r.row_count,
                        started.elapsed().as_millis() as u64,
                        Some(view.label().into()),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出 JSON",
                        dest.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        1,
                        1,
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    self.error = Some(msg);
                }
            }
        }
    }

    fn export_comparison(&mut self) {
        let Some(cmp) = self.comparison.clone() else {
            return;
        };
        if let Some(dest) = rfd::FileDialog::new()
            .set_file_name("imatest_compare.csv")
            .save_file()
        {
            let started = Instant::now();
            match DataExportService::export_comparison(&cmp, &dest) {
                Ok(r) => {
                    self.status_hint =
                        format!("已导出对比 CSV：{} 行 → {}", r.row_count, r.dest.display());
                    self.output.status_message = self.status_hint.clone();
                    self.record_action(
                        "导出对比 CSV",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.row_count,
                        0,
                        r.row_count,
                        started.elapsed().as_millis() as u64,
                        None,
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出对比 CSV",
                        dest.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        1,
                        1,
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    self.error = Some(msg);
                }
            }
        }
    }

    fn export_html_report(&mut self) {
        if let Some(dest) = rfd::FileDialog::new()
            .set_file_name("imatest_report.html")
            .save_file()
        {
            let started = Instant::now();
            let raw_table = match self.active_table_view() {
                DataTableView::Summary => self.summary_table(),
                DataTableView::Detail => SummaryService::detail_table(&self.batches),
                DataTableView::Compare => self.summary_table(),
            };
            let table = self.table_with_overrides(&raw_table);
            let schema = self.schema_for_table(&table);
            let insights = self.current_insights(&table);
            match DataExportService::export_summary_html_report(&table, &schema, &insights, &dest) {
                Ok(r) => {
                    self.status_hint = format!(
                        "已导出 HTML 报告：{} 行 → {}",
                        r.row_count,
                        r.dest.display()
                    );
                    self.output.status_message = self.status_hint.clone();
                    self.record_action(
                        "导出 HTML 报告",
                        r.dest.display().to_string(),
                        ActionHistoryStatus::Succeeded,
                        r.row_count,
                        0,
                        r.row_count,
                        started.elapsed().as_millis() as u64,
                        Some("包含洞察摘要和当前导出列".into()),
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.record_action(
                        "导出 HTML 报告",
                        dest.display().to_string(),
                        ActionHistoryStatus::Failed,
                        0,
                        1,
                        1,
                        started.elapsed().as_millis() as u64,
                        Some(msg.clone()),
                    );
                    self.error = Some(msg);
                }
            }
        }
    }

    fn has_export_rows(&self) -> bool {
        match self.active_table_view() {
            DataTableView::Summary => self.batches.iter().any(|b| !b.records.is_empty()),
            DataTableView::Detail => self.batches.iter().any(|b| !b.records.is_empty()),
            DataTableView::Compare => self
                .comparison
                .as_ref()
                .map(|c| !c.rows.is_empty())
                .unwrap_or(false),
        }
    }

    fn mark_summary_dirty(&mut self) {
        self.summary_dirty = true;
        self.summary_cache = None;
    }

    fn summary_table(&mut self) -> SummaryTable {
        if self.summary_dirty || self.summary_cache.is_none() {
            self.summary_cache = Some(SummaryService::build(&self.batches));
            self.summary_dirty = false;
        }
        self.summary_cache.clone().unwrap_or_default()
    }

    fn recent_tasks_ui(&mut self, ui: &mut egui::Ui) {
        let recent: Vec<_> = self
            .action_history
            .iter()
            .filter(|entry| entry.module == "数据提取")
            .take(5)
            .cloned()
            .collect();
        widgets::grouped_section(ui, "最近任务", |ui| {
            if recent.is_empty() {
                ui.label(RichText::new("暂无任务记录").small().weak());
                return;
            }
            for entry in recent {
                ui.label(
                    RichText::new(format!(
                        "{} · {} · 成功 {} / 失败 {}",
                        entry.operation,
                        entry.status.label(),
                        entry.success_count,
                        entry.failure_count
                    ))
                    .size(11.0),
                );
                ui.label(RichText::new(entry.target).small().weak());
                if let Some(detail) = entry.detail {
                    ui.label(
                        RichText::new(detail.lines().next().unwrap_or_default())
                            .small()
                            .weak(),
                    );
                }
                ui.add_space(4.0);
            }
        });
    }

    fn insights_ui(&mut self, ui: &mut egui::Ui) {
        let table = self.summary_table();
        let insights = self.current_insights(&table);
        widgets::grouped_section(ui, "洞察", |ui| {
            if insights.summary.is_empty() {
                ui.label(RichText::new("暂无洞察，导入数据后自动生成").small().weak());
                return;
            }
            for line in insights.summary.iter().take(5) {
                ui.label(RichText::new(line).small());
            }
            if !insights.outliers.is_empty() {
                if widgets::compact_secondary_button(ui, "筛选离群点", true).clicked() {
                    self.search_buf = "outlier:true".into();
                    self.table_view = DataTableView::Summary;
                }
                for outlier in insights.outliers.iter().take(3) {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} = {:.4}",
                            outlier.sample, outlier.metric, outlier.value
                        ))
                        .small()
                        .weak(),
                    );
                }
            }
        });
    }

    fn current_insights(&self, table: &SummaryTable) -> DataInsightReport {
        DataInsightService::analyze(&self.batches, table)
    }

    fn record_action(
        &mut self,
        operation: impl Into<String>,
        target: impl Into<String>,
        status: ActionHistoryStatus,
        success_count: usize,
        failure_count: usize,
        total_count: usize,
        elapsed_ms: u64,
        detail: Option<String>,
    ) {
        let entry = ActionHistoryEntry {
            finished_at_unix: prefs::now_unix(),
            module: "数据提取".into(),
            operation: operation.into(),
            target: target.into(),
            status,
            success_count,
            failure_count,
            total_count,
            elapsed_ms,
            detail,
        };
        let mut prefs = GuiPrefs::load();
        prefs.push_action_history(entry);
        self.action_history = prefs.action_history.clone();
        let _ = prefs.save();
    }

    fn remote_results_source_ui(&mut self, ui: &mut egui::Ui) {
        if !crate::remote::remote_enabled(&self.remote_config) {
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.label("数据源");
            if ui
                .selectable_label(
                    self.data_source == crate::remote::DataSource::Remote,
                    crate::remote::DataSource::Remote.label(),
                )
                .clicked()
            {
                if self.data_source != crate::remote::DataSource::Remote {
                    self.data_source = crate::remote::DataSource::Remote;
                    self.start_remote_results_fetch();
                }
            }
            if ui
                .selectable_label(
                    self.data_source == crate::remote::DataSource::Local,
                    crate::remote::DataSource::Local.label(),
                )
                .clicked()
            {
                if self.data_source != crate::remote::DataSource::Local {
                    self.switch_to_local("已切换到本地结果");
                }
            }
            if widgets::compact_secondary_button(
                ui,
                "刷新远程",
                self.data_source == crate::remote::DataSource::Remote && !self.remote_loading,
            )
            .clicked()
            {
                self.start_remote_results_fetch();
            }
        });

        if self.remote_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("远程加载中…").small().weak());
            });
        }

        if self.data_source != crate::remote::DataSource::Remote {
            return;
        }

        ui.add_space(4.0);
        ui.label(RichText::new("远程结果").strong());
        if self.remote_results.is_empty() {
            ui.label(RichText::new("暂无远程结果").small().weak());
            return;
        }

        ScrollArea::vertical()
            .id_salt("data_extract_remote_results")
            .max_height(130.0)
            .show(ui, |ui| {
                for result in self.remote_results.clone() {
                    let label = format!(
                        "{}\n{} · 更新 {}",
                        result.batch_name, result.module, result.updated_at
                    );
                    let enabled = result.report_asset.is_some() && !self.remote_loading;
                    let clicked = ui
                        .add_enabled(enabled, egui::Button::new(label).frame(false))
                        .clicked();
                    if clicked {
                        self.start_remote_report_fetch(result.clone());
                    }
                    if !enabled && result.report_asset.is_none() {
                        ui.label(RichText::new("无可下载报告").small().weak());
                    }
                    ui.add_space(3.0);
                }
            });
    }

    fn start_remote_results_fetch(&mut self) {
        if !crate::remote::remote_enabled(&self.remote_config) {
            self.switch_to_local("远程未配置，已回退本地");
            return;
        }
        let cfg = self.remote_config.clone();
        self.remote_loading = true;
        self.results_fetch = Some(crate::remote::RemoteFetch::spawn(move || {
            let _ = crate::remote::probe_remote_health(&cfg)?;
            crate::remote::list_remote_extract_results(&cfg)
        }));
    }

    fn start_remote_report_fetch(&mut self, result: crate::remote::RemoteExtractResultSummary) {
        let Some(asset) = result.report_asset.clone() else {
            self.status_hint = "远程结果没有可下载报告".into();
            self.output.status_message = self.status_hint.clone();
            return;
        };
        let cfg = self.remote_config.clone();
        let result_id = result.result_id.clone();
        self.remote_loading = true;
        self.report_fetch = Some(crate::remote::RemoteFetch::spawn(move || {
            let local_path = crate::remote::ensure_remote_asset_local(&cfg, &asset)?;
            Ok((result_id, local_path))
        }));
    }

    fn poll_remote_fetches(&mut self, ctx: &egui::Context) {
        let results_result = self.results_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = results_result {
            self.results_fetch = None;
            self.remote_loading = self.report_fetch.is_some();
            match result {
                Ok(results) => {
                    self.remote_results = results;
                    self.status_hint =
                        format!("已加载 {} 个远程数据提取结果", self.remote_results.len());
                    self.output.status_message = self.status_hint.clone();
                }
                Err(e) => self.switch_to_local(format!("远程结果加载失败，已回退本地：{e}")),
            }
            ctx.request_repaint();
        } else if self.results_fetch.is_some() {
            ctx.request_repaint();
        }

        let report_result = self.report_fetch.as_ref().and_then(|fetch| fetch.poll());
        if let Some(result) = report_result {
            self.report_fetch = None;
            self.remote_loading = self.results_fetch.is_some();
            match result {
                Ok((result_id, path)) => self.import_remote_report(&result_id, path),
                Err(e) => {
                    self.status_hint = format!("远程报告下载失败：{e}");
                    self.output.status_message = self.status_hint.clone();
                }
            }
            ctx.request_repaint();
        } else if self.report_fetch.is_some() {
            ctx.request_repaint();
        }
    }

    fn import_remote_report(&mut self, result_id: &str, path: PathBuf) {
        let batch_name = self
            .remote_results
            .iter()
            .find(|result| result.result_id == result_id)
            .map(|result| result.batch_name.clone())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| result_id.to_string())
            });

        match DataExtractService::extract_with_thresholds(&path, Some(&self.threshold_profile)) {
            Ok(mut batch) => {
                batch.name = format!("[远程] {batch_name}");
                let total = batch.records.len();
                self.batch_rename_buf = batch.name.clone();
                self.batches.insert(0, batch);
                self.current_batch = Some(0);
                self.selected_record = None;
                self.comparison = None;
                self.table_view = DataTableView::Summary;
                self.mark_summary_dirty();
                self.status_hint = format!("已打开远程结果：{batch_name}（{total} 条指标）");
                self.output.status_message = self.status_hint.clone();
                self.record_action(
                    "打开远程结果",
                    batch_name,
                    ActionHistoryStatus::Succeeded,
                    total,
                    0,
                    total,
                    0,
                    Some(path.display().to_string()),
                );
            }
            Err(e) => {
                let msg = format!("远程报告解析失败：{e}");
                self.record_action(
                    "打开远程结果",
                    result_id.to_string(),
                    ActionHistoryStatus::Failed,
                    0,
                    1,
                    1,
                    0,
                    Some(msg.clone()),
                );
                self.error = Some(msg);
            }
        }
    }

    fn switch_to_local(&mut self, reason: impl Into<String>) {
        self.data_source = crate::remote::DataSource::Local;
        self.results_fetch = None;
        self.report_fetch = None;
        self.remote_loading = false;
        self.status_hint = reason.into();
        self.output.status_message = self.status_hint.clone();
    }

    fn submit_remote_data_extract(
        &mut self,
        paths: &[PathBuf],
        batch_name: &str,
    ) -> Option<String> {
        if !crate::remote::remote_enabled(&self.remote_config) {
            return None;
        }
        let candidates = data_extract_candidate_paths(paths);
        if candidates.is_empty() {
            return Some("远程数据提取跳过：无候选文件".into());
        }
        let assets = match crate::remote::upload_paths_as_assets(&self.remote_config, &candidates) {
            Ok(assets) => assets,
            Err(e) => return Some(format!("远程数据提取上传失败：{e}")),
        };
        let extras = vec![
            ("batch_name".into(), batch_name.to_string()),
            ("module".into(), "imatest".into()),
            ("export_formats".into(), "csv,json,html".into()),
        ];
        match crate::remote::submit_module_job(
            &self.remote_config,
            crate::remote::RemoteJobSource::DataExtract,
            assets,
            extras,
        ) {
            Ok((status, result)) => {
                let mut note = format!("远程数据提取任务 {}", status.job_id);
                if result.phase == crate::remote::RemoteJobPhase::Succeeded {
                    note = format!("远程数据提取完成 {}", status.job_id);
                }
                if let Some(report) = result.artifacts.first() {
                    match self.download_remote_extract_report(report) {
                        Ok(path) => note.push_str(&format!(" · 报告 {}", path.display())),
                        Err(e) => note.push_str(&format!(" · 报告下载失败：{e}")),
                    }
                }
                self.start_remote_results_fetch();
                Some(note)
            }
            Err(e) => Some(format!("远程数据提取提交失败：{e}")),
        }
    }

    fn download_remote_extract_report(
        &self,
        asset: &crate::remote::RemoteAssetRef,
    ) -> crate::remote::RemoteResult<PathBuf> {
        let client = crate::remote::try_build_http_client(&self.remote_config)?;
        let service = crate::remote::RemoteAssetService::new(self.remote_config.clone(), client);
        let file_name = std::path::Path::new(&asset.name)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.csv", asset.id));
        let dest = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("remote_data_extract_reports")
            .join(file_name);
        service.download_to(&asset.id, &dest)
    }

    fn current_batch(&self) -> Option<&ExtractionBatch> {
        self.current_batch.and_then(|i| self.batches.get(i))
    }

    fn selected_record(&self) -> Option<&ExtractionRecord> {
        let batch = self.current_batch()?;
        let idx = self.selected_record?;
        batch.records.get(idx)
    }

    fn filtered_detail_records(&self) -> Vec<(usize, usize, String, ExtractionRecord)> {
        let search = self.search_buf.to_ascii_lowercase();
        let mut out = Vec::new();
        for (batch_idx, batch) in self.batches.iter().enumerate() {
            for (record_idx, rec) in batch.records.iter().enumerate() {
                if let Some(m) = self.module_filter {
                    if rec.module != m {
                        continue;
                    }
                }
                if let Some(st) = self.status_filter {
                    if rec.evaluation_status() != st {
                        continue;
                    }
                }
                if !search.is_empty()
                    && !rec.metric_key.to_ascii_lowercase().contains(&search)
                    && !rec.raw_name.to_ascii_lowercase().contains(&search)
                    && !rec
                        .value
                        .display_value()
                        .to_ascii_lowercase()
                        .contains(&search)
                    && !batch.name.to_ascii_lowercase().contains(&search)
                {
                    continue;
                }
                out.push((batch_idx, record_idx, batch.name.clone(), rec.clone()));
            }
        }
        out
    }
}

fn fixed_grouped_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    outer_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    widgets::section_header(ui, title);
    ui.add_space(6.0);

    let dark = ui.style().visuals.dark_mode;
    let inner_width = (outer_width - 32.0).max(120.0);
    Frame::new()
        .fill(theme::grouped_fill(dark))
        .corner_radius(CornerRadius::same(theme::GROUP_RADIUS))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(inner_width);
            ui.set_max_width(inner_width);
            add_contents(ui)
        })
        .inner
}

fn summary_row_storage_key(row: &crate::data_extract::domain::SummaryRow) -> String {
    format!(
        "{}|{}|{}",
        row.batch_id,
        row.sample_name,
        row.source_path.to_string_lossy()
    )
}

fn data_extract_candidate_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = std::collections::BTreeSet::new();
    for path in paths {
        for candidate in crate::data_extract::service::scanner::scan_directory(path) {
            out.insert(candidate);
        }
    }
    out.into_iter().collect()
}

fn status_color(status: EvaluationStatus, dark: bool) -> Color32 {
    match status {
        EvaluationStatus::Pass => theme::success_color(dark),
        EvaluationStatus::Warn => theme::warning_color(dark),
        EvaluationStatus::Fail => theme::error_color(dark),
        EvaluationStatus::Unknown => theme::secondary_label(dark),
    }
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|n| format!("{n:.4}")).unwrap_or_else(|| "—".into())
}
