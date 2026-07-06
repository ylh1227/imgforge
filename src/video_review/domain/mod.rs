//! 视频评审领域模型。

pub mod batch;
pub mod marker;
pub mod metadata;
pub mod segment;
pub mod tag;
pub mod video_item;

pub use batch::{BatchStats, VideoBatch};
pub use marker::{MarkerKind, VideoMarker};
pub use metadata::VideoMetadata;
pub use segment::VideoSegment;
pub use tag::VideoTag;
pub use video_item::{is_video_extension, VideoFilter, VideoItem, VIDEO_EXTENSIONS};
