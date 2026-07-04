//! egui 评审面板组件。

pub mod annotation_canvas;
mod canvas;
mod compare_view;
mod review_panel;
mod shortcuts;
mod sidebar;
mod texture_cache;
mod toolbar;

pub use annotation_canvas::AnnotationCanvas;
pub use compare_view::{CompareDisplayMode, CompareView, CompareViewConfig, SplitLayout};
pub use review_panel::{ReviewPanel, ReviewPanelHost, ReviewPanelOutput};
