//! 直连 JIRA REST：探活、建单、上传附件（含 429/5xx 重试）。

use std::fs::File;
use std::path::Path;
use std::thread;
use std::time::Duration;

use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::jira::config::{JiraAuthMode, JiraConfig};
use crate::jira::error::{JiraError, JiraResult};
use crate::jira::mapping::{attachment_filename, build_create_fields, MappedIssue};

const MAX_RETRIES: u32 = 3;
const RETRY_BASE_MS: u64 = 250;
const USER_AGENT_VALUE: &str = concat!("imgforge/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct CreatedIssue {
    pub key: String,
    pub id: String,
    pub self_url: Option<String>,
    pub browse_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateIssueResponse {
    key: String,
    id: String,
    #[serde(rename = "self")]
    self_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MyselfResponse {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JiraMyself {
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct JiraClient {
    http: Client,
    config: JiraConfig,
    base_url: String,
}

impl JiraClient {
    pub fn try_new(config: &JiraConfig) -> JiraResult<Self> {
        if !config.enabled {
            return Err(JiraError::Disabled);
        }
        let Some(base) = config
            .base_url
            .as_ref()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
        else {
            return Err(JiraError::NotConfigured("缺少 jira.base_url".into()));
        };
        if config
            .project_key
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            return Err(JiraError::NotConfigured("缺少 jira.project_key".into()));
        }
        if !config.has_credentials() {
            return Err(JiraError::Auth(match config.auth_mode {
                JiraAuthMode::EnvBasic => {
                    "未设置 IMGFORGE_JIRA_EMAIL / IMGFORGE_JIRA_API_TOKEN".into()
                }
                JiraAuthMode::EnvBearer => "未设置 IMGFORGE_JIRA_PAT".into(),
            }));
        }

        let http = Client::builder()
            .timeout(config.timeout())
            .user_agent(USER_AGENT_VALUE)
            .build()
            .map_err(|e| JiraError::Network(e.to_string()))?;

        Ok(Self {
            http,
            config: config.clone(),
            base_url: base,
        })
    }

    pub fn config(&self) -> &JiraConfig {
        &self.config
    }

    fn api_root(&self) -> String {
        format!(
            "{}/rest/api/{}",
            self.base_url,
            self.config.api_version.path_segment()
        )
    }

    fn apply_auth(&self, builder: RequestBuilder) -> RequestBuilder {
        match self.config.auth_mode {
            JiraAuthMode::EnvBasic => {
                if let Some((email, token)) = self.config.resolve_basic_credentials() {
                    builder.basic_auth(email, Some(token))
                } else {
                    builder
                }
            }
            JiraAuthMode::EnvBearer => {
                if let Some(token) = self.config.resolve_bearer_token() {
                    builder.header(AUTHORIZATION, format!("Bearer {token}"))
                } else {
                    builder
                }
            }
        }
    }

    fn send_with_retry(
        &self,
        _method: Method,
        _url: &str,
        build: impl Fn() -> RequestBuilder,
    ) -> JiraResult<Response> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let builder = self.apply_auth(build().header(USER_AGENT, USER_AGENT_VALUE));
            let result = builder.send();
            match result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        let message = resp.text().unwrap_or_default();
                        return Err(JiraError::Auth(truncate_msg(&message)));
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if retryable && attempt < MAX_RETRIES {
                        let sleep_ms = RETRY_BASE_MS.saturating_mul(1u64 << (attempt - 1));
                        thread::sleep(Duration::from_millis(sleep_ms));
                        continue;
                    }
                    if !status.is_success() {
                        let message = resp.text().unwrap_or_default();
                        return Err(JiraError::Api {
                            status: status.as_u16(),
                            message: truncate_msg(&message),
                        });
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let sleep_ms = RETRY_BASE_MS.saturating_mul(1u64 << (attempt - 1));
                        thread::sleep(Duration::from_millis(sleep_ms));
                        continue;
                    }
                    return Err(JiraError::Network(e.to_string()));
                }
            }
        }
    }

    pub fn myself(&self) -> JiraResult<JiraMyself> {
        let url = format!("{}/myself", self.api_root());
        let resp = self.send_with_retry(Method::GET, &url, || self.http.get(&url))?;
        let body: MyselfResponse = resp
            .json()
            .map_err(|e| JiraError::Parse(e.to_string()))?;
        let display_name = body
            .display_name
            .or(body.name)
            .or(body.account_id)
            .unwrap_or_else(|| "(unknown)".into());
        Ok(JiraMyself { display_name })
    }

    pub fn create_issue(&self, mapped: &MappedIssue) -> JiraResult<CreatedIssue> {
        let url = format!("{}/issue", self.api_root());
        let fields = build_create_fields(&self.config, mapped);
        let payload = json!({ "fields": fields });
        let resp = self.send_with_retry(Method::POST, &url, || {
            self.http
                .post(&url)
                .header(CONTENT_TYPE, "application/json")
                .json(&payload)
        })?;
        let body: CreateIssueResponse = resp
            .json()
            .map_err(|e| JiraError::Parse(e.to_string()))?;
        Ok(CreatedIssue {
            key: body.key.clone(),
            id: body.id,
            self_url: body.self_url,
            browse_url: self.config.issue_browse_url(&body.key),
        })
    }

    pub fn attach_file(&self, issue_key: &str, path: &Path) -> JiraResult<()> {
        let meta = std::fs::metadata(path)?;
        let size = meta.len();
        if size > self.config.max_attach_bytes {
            return Err(JiraError::AttachmentTooLarge {
                path: path.display().to_string(),
                size,
                limit: self.config.max_attach_bytes,
            });
        }
        let url = format!("{}/issue/{issue_key}/attachments", self.api_root());
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            // multipart 需重新打开文件
            let file = File::open(path)?;
            let part = Part::reader(file)
                .file_name(attachment_filename(path))
                .mime_str("application/octet-stream")
                .map_err(|e| JiraError::Other(e.to_string()))?;
            let form = Form::new().part("file", part);
            let builder = self
                .apply_auth(self.http.post(&url))
                .header(USER_AGENT, USER_AGENT_VALUE)
                .header("X-Atlassian-Token", "no-check")
                .multipart(form);
            match builder.send() {
                Ok(resp) => {
                    let status = resp.status();
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        let message = resp.text().unwrap_or_default();
                        return Err(JiraError::Auth(truncate_msg(&message)));
                    }
                    let retryable = status.as_u16() == 429 || status.is_server_error();
                    if retryable && attempt < MAX_RETRIES {
                        let sleep_ms = RETRY_BASE_MS.saturating_mul(1u64 << (attempt - 1));
                        thread::sleep(Duration::from_millis(sleep_ms));
                        continue;
                    }
                    if !status.is_success() {
                        let message = resp.text().unwrap_or_default();
                        return Err(JiraError::Api {
                            status: status.as_u16(),
                            message: truncate_msg(&message),
                        });
                    }
                    return Ok(());
                }
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        let sleep_ms = RETRY_BASE_MS.saturating_mul(1u64 << (attempt - 1));
                        thread::sleep(Duration::from_millis(sleep_ms));
                        continue;
                    }
                    return Err(JiraError::Network(e.to_string()));
                }
            }
        }
    }

    /// 探测连通性（供设置页 / doctor）。
    pub fn probe(config: &JiraConfig) -> JiraResult<JiraMyself> {
        Self::try_new(config)?.myself()
    }
}

fn truncate_msg(s: &str) -> String {
    const MAX: usize = 400;
    let t = s.trim();
    if t.chars().count() <= MAX {
        t.to_string()
    } else {
        let mut out: String = t.chars().take(MAX).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use httpmock::prelude::*;
    use tempfile::NamedTempFile;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_basic_creds<R>(f: impl FnOnce() -> R) -> R {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("IMGFORGE_JIRA_EMAIL", "tester@example.com");
        std::env::set_var("IMGFORGE_JIRA_API_TOKEN", "test-token");
        f()
    }

    fn test_cfg(server: &MockServer) -> JiraConfig {
        JiraConfig {
            enabled: true,
            base_url: Some(server.base_url()),
            project_key: Some("CAM".into()),
            auth_mode: JiraAuthMode::EnvBasic,
            timeout_secs: 10,
            max_attach_bytes: 1024,
            ..JiraConfig::default()
        }
    }

    fn sample_mapped() -> MappedIssue {
        MappedIssue {
            summary: "test bug".into(),
            description_text: "desc".into(),
            priority: "Medium".into(),
            labels: vec!["imgforge".into()],
        }
    }

    #[test]
    fn try_new_requires_enabled() {
        let cfg = JiraConfig::default();
        assert!(matches!(JiraClient::try_new(&cfg), Err(JiraError::Disabled)));
    }

    #[test]
    fn try_new_requires_credentials() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IMGFORGE_JIRA_EMAIL");
        std::env::remove_var("IMGFORGE_JIRA_API_TOKEN");
        std::env::remove_var("IMGFORGE_JIRA_PAT");
        let cfg = JiraConfig {
            enabled: true,
            base_url: Some("https://example.atlassian.net".into()),
            project_key: Some("CAM".into()),
            ..JiraConfig::default()
        };
        assert!(matches!(JiraClient::try_new(&cfg), Err(JiraError::Auth(_))));
    }

    #[test]
    fn create_issue_parses_key() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(201).json_body(serde_json::json!({
                    "id": "10001",
                    "key": "CAM-42",
                    "self": format!("{}/rest/api/3/issue/10001", server.base_url())
                }));
            });
            let client = JiraClient::try_new(&test_cfg(&server)).unwrap();
            let created = client.create_issue(&sample_mapped()).unwrap();
            mock.assert();
            assert_eq!(created.key, "CAM-42");
            assert!(created.browse_url.unwrap().ends_with("/browse/CAM-42"));
        });
    }

    #[test]
    fn create_issue_retries_then_fails_on_persistent_429() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(429).body("rate limited");
            });
            let client = JiraClient::try_new(&test_cfg(&server)).unwrap();
            let err = client.create_issue(&sample_mapped()).unwrap_err();
            assert!(matches!(err, JiraError::Api { status: 429, .. }));
            assert!(mock.hits() >= 2);
        });
    }

    #[test]
    fn auth_failure_is_detected() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST).path("/rest/api/3/issue");
                then.status(401).body("unauthorized");
            });
            let client = JiraClient::try_new(&test_cfg(&server)).unwrap();
            let err = client.create_issue(&sample_mapped()).unwrap_err();
            mock.assert();
            assert!(err.is_auth_failure());
        });
    }

    #[test]
    fn attach_file_sends_multipart_and_token_header() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let mock = server.mock(|when, then| {
                when.method(POST)
                    .path("/rest/api/3/issue/CAM-9/attachments")
                    .header("X-Atlassian-Token", "no-check");
                then.status(200).body("[]");
            });
            let mut tmp = NamedTempFile::new().unwrap();
            use std::io::Write;
            write!(tmp, "hello").unwrap();
            let client = JiraClient::try_new(&test_cfg(&server)).unwrap();
            client.attach_file("CAM-9", tmp.path()).unwrap();
            mock.assert();
        });
    }

    #[test]
    fn attach_too_large_rejected() {
        with_basic_creds(|| {
            let server = MockServer::start();
            let client = JiraClient::try_new(&test_cfg(&server)).unwrap();
            let mut tmp = NamedTempFile::new().unwrap();
            use std::io::Write;
            let big = vec![0u8; 2048];
            tmp.write_all(&big).unwrap();
            let err = client.attach_file("CAM-9", tmp.path()).unwrap_err();
            assert!(matches!(err, JiraError::AttachmentTooLarge { .. }));
        });
    }
}
