//! 片段备注。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::review::domain::image_item::ReviewStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSegment {
    pub id: i64,
    pub video_id: i64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub status: ReviewStatus,
    pub created_at: DateTime<Utc>,
}
