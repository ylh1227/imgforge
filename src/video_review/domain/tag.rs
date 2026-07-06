//! 视频标签。

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct VideoTag {
  pub id: i64,
  pub name: String,
  pub color: [u8; 4],
  pub created_at: DateTime<Utc>,
}

impl VideoTag {
  pub const PALETTE: [[u8; 4]; 8] = [
    [0, 122, 255, 255],
    [255, 45, 85, 255],
    [52, 199, 89, 255],
    [255, 149, 0, 255],
    [175, 82, 222, 255],
    [90, 200, 250, 255],
    [255, 204, 0, 255],
    [142, 142, 147, 255],
  ];
}
