//! 视频评审持久化层。

pub mod migrate;
pub mod paths;
pub mod repository;
pub mod sqlite_repository;

pub use repository::{NewVideoItem, VideoRepository};
pub use sqlite_repository::SqliteVideoRepository;
