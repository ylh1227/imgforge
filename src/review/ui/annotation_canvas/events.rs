//! 画布向外抛出的事件（单向数据流，不直接改外部状态）。

use super::types::{AnnotationKind, AnnotationPosition, AnnotationStyle, CanvasTool};

/// 用户操作事件：由上层 service 持久化或更新状态。
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationCanvasEvent {
    /// 创建新标注（上层分配 id 后写回 `annotations` 切片）。
    CreateAnnotation {
        kind: AnnotationKind,
        position: AnnotationPosition,
        style: AnnotationStyle,
        content: String,
    },
    /// 更新标注几何位置。
    UpdateAnnotation {
        id: i64,
        position: AnnotationPosition,
    },
    /// 更新文字内容。
    UpdateAnnotationContent { id: i64, content: String },
    /// 删除标注。
    DeleteAnnotation { id: i64 },
    /// 选中变化（可选监听）。
    SelectionChanged { id: Option<i64> },
    /// 工具切换（可选监听）。
    ToolChanged { tool: CanvasTool },
}
