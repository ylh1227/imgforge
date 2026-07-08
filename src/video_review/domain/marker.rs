//! 时间点标记。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerKind {
    Issue,
    Note,
    Sync,
}

impl MarkerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Issue => "问题",
            Self::Note => "备注",
            Self::Sync => "同步点",
        }
    }

    pub fn to_sql(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::Note => "note",
            Self::Sync => "sync",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        match s {
            "issue" => Some(Self::Issue),
            "note" => Some(Self::Note),
            "sync" => Some(Self::Sync),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMarker {
    pub id: i64,
    pub video_id: i64,
    pub time_ms: u64,
    pub kind: MarkerKind,
    pub text: String,
    pub severity: u8,
    pub created_at: DateTime<Utc>,
}
