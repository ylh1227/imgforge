//! 评审面板公开类型与内部对话框状态。

use std::path::PathBuf;

use crate::review::domain::image_item::ReviewStatus;
use crate::review::service::StatusTransitionWarning;

/// 主应用向评审面板提供的上下文（评审模块不依赖 gui 内部实现）。
pub trait ReviewPanelHost {
    /// 格式转换页待处理/已导入的路径队列。
    fn conversion_queue_paths(&self) -> &[PathBuf];
    /// 转换输出目录（用于对比视图查找转换后预览）。
    fn output_directory(&self) -> &str;
}

/// 评审面板向主应用输出的联动指令。
#[derive(Debug, Clone, Default)]
pub struct ReviewPanelOutput {
    /// 将「通过」的图片路径加入格式转换队列。
    pub enqueue_approved: Vec<PathBuf>,
    /// 单图转换参数覆盖（评审标记带入队列），与 `enqueue_approved` 对应。
    pub enqueue_params: Vec<crate::review::ConversionTaskParams>,
    pub status_message: String,
    /// 请求主应用切回格式转换 Tab。
    pub switch_to_convert: bool,
    /// 请求打开指定远程评审批次。
    pub open_remote_batch_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RightTab {
    #[default]
    Review,
    Info,
    Analysis,
    Annotations,
    Tags,
}

impl RightTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Review => "评审属性",
            Self::Info => "图片信息",
            Self::Analysis => "分析",
            Self::Annotations => "标注列表",
            Self::Tags => "标签",
        }
    }

    pub(crate) fn all() -> [Self; 5] {
        [
            Self::Review,
            Self::Info,
            Self::Analysis,
            Self::Annotations,
            Self::Tags,
        ]
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DialogState {
    ConfirmBatchOp(BatchOpKind),
    IrreversibleStatus {
        target: ReviewStatus,
        warnings: Vec<StatusTransitionWarning>,
        confirm: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BatchOpKind {
    SetStatus(ReviewStatus),
    ClearAnnotations,
    AddRemark,
    CopyCurrentAnnotations,
}
