//! 数据面：上传会话、预签名 URL、artifact 下载凭证。

use serde::{Deserialize, Serialize};

use crate::remote::types::{now_unix, RemoteAssetRef, REMOTE_SCHEMA_VERSION};

/// 上传协议偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteUploadProtocol {
    /// S3/MinIO 预签名 PUT（单对象或 multipart）。
    #[default]
    PresignedPut,
    /// S3 multipart 分片上传。
    S3Multipart,
    /// tus 断点续传（后续阶段）。
    Tus,
}

/// 初始化上传请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteUploadInitRequest {
    pub schema_version: u32,
    pub file_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default)]
    pub protocol: RemoteUploadProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
}

impl Default for RemoteUploadInitRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            file_name: String::new(),
            mime: None,
            size: None,
            checksum: None,
            protocol: RemoteUploadProtocol::PresignedPut,
            workspace_id: None,
        }
    }
}

/// 单个可 PUT 的分片/对象 URL。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePresignedPart {
    pub part_number: u32,
    pub upload_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<(String, String)>>,
}

/// 上传会话（init 响应）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteUploadSession {
    pub schema_version: u32,
    pub upload_id: String,
    pub protocol: RemoteUploadProtocol,
    /// 建议分片大小（字节）；单 PUT 时可等于整个对象。
    pub part_size: u64,
    pub parts: Vec<RemotePresignedPart>,
    /// 会话过期 Unix 秒。
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
}

impl RemoteUploadSession {
    pub fn single_put(
        upload_id: impl Into<String>,
        upload_url: impl Into<String>,
        part_size: u64,
        ttl_secs: u64,
    ) -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            upload_id: upload_id.into(),
            protocol: RemoteUploadProtocol::PresignedPut,
            part_size,
            parts: vec![RemotePresignedPart {
                part_number: 1,
                upload_url: upload_url.into(),
                headers: None,
            }],
            expires_at: now_unix().saturating_add(ttl_secs),
            object_key: None,
        }
    }
}

/// 完成上传请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteUploadCompleteRequest {
    pub schema_version: u32,
    pub upload_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// multipart 时各分片 ETag。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub part_etags: Vec<(u32, String)>,
}

impl Default for RemoteUploadCompleteRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            upload_id: String::new(),
            checksum: None,
            part_etags: Vec::new(),
        }
    }
}

/// 完成上传响应：得到可引用的素材。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteUploadCompleteResponse {
    pub schema_version: u32,
    pub asset: RemoteAssetRef,
}

/// 中止上传。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteUploadAbortRequest {
    pub schema_version: u32,
    pub upload_id: String,
}

impl Default for RemoteUploadAbortRequest {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_SCHEMA_VERSION,
            upload_id: String::new(),
        }
    }
}

/// Artifact / 素材下载凭证。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDownloadCredential {
    pub schema_version: u32,
    pub asset_id: String,
    pub download_url: String,
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_put_session_has_one_part() {
        let s = RemoteUploadSession::single_put("u1", "https://store/x", 1024, 60);
        assert_eq!(s.parts.len(), 1);
        assert_eq!(s.protocol, RemoteUploadProtocol::PresignedPut);
    }
}
