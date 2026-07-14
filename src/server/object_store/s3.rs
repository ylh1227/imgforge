//! S3 / MinIO 兼容对象存储（AWS SigV4 + path-style）。

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{ObjectResult, ObjectStore};
use crate::server::config::S3Config;
use crate::server::storage::StoreError;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct S3ObjectStore {
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    path_style: bool,
    public_base: String,
    http: reqwest::blocking::Client,
}

impl S3ObjectStore {
    pub fn from_config(s3: &S3Config, public_base: impl Into<String>) -> Result<Self, String> {
        let endpoint = s3
            .endpoint
            .clone()
            .ok_or_else(|| "s3.endpoint required".to_string())?;
        let bucket = s3
            .bucket
            .clone()
            .ok_or_else(|| "s3.bucket required".to_string())?;
        let access_key = s3
            .access_key
            .clone()
            .ok_or_else(|| "s3.access_key required".to_string())?;
        let secret_key = s3
            .secret_key
            .clone()
            .ok_or_else(|| "s3.secret_key required".to_string())?;
        let region = s3.region.clone().unwrap_or_else(|| "us-east-1".into());
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            region,
            bucket,
            access_key,
            secret_key,
            path_style: s3.path_style,
            public_base: public_base.into(),
            http,
        })
    }

    fn object_url(&self, key: &str) -> String {
        let key = key.trim_start_matches('/');
        if self.path_style {
            format!("{}/{}/{}", self.endpoint, self.bucket, key)
        } else {
            // virtual-hosted：把 bucket 塞进 host 较复杂，生产默认 path-style（MinIO）
            format!("{}/{}/{}", self.endpoint, self.bucket, key)
        }
    }

    fn host_header(&self) -> String {
        let without_scheme = self
            .endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        if self.path_style {
            without_scheme.to_string()
        } else {
            format!("{}.{}", self.bucket, without_scheme)
        }
    }

    fn sign_and_send(
        &self,
        method: &str,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> ObjectResult<reqwest::blocking::Response> {
        let url = self.object_url(key);
        let host = self.host_header();
        let amz_date = amz_date_now();
        let date_stamp = &amz_date[..8];
        let payload_hash = hex::encode(Sha256::digest(body));
        let canonical_uri = if self.path_style {
            format!("/{}/{}", self.bucket, key.trim_start_matches('/'))
        } else {
            format!("/{}", key.trim_start_matches('/'))
        };
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            hex::encode(Sha256::digest(canonical_request.as_bytes()))
        );
        let signing_key = derive_signing_key(&self.secret_key, date_stamp, &self.region, "s3");
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let mut builder = match method {
            "PUT" => self.http.put(&url),
            "GET" => self.http.get(&url),
            "DELETE" => self.http.delete(&url),
            "HEAD" => self.http.head(&url),
            other => {
                return Err(StoreError::Internal(format!("unsupported method {other}")));
            }
        };
        builder = builder
            .header("host", &host)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("authorization", authorization);
        if let Some(ct) = content_type {
            builder = builder.header("content-type", ct);
        }
        if matches!(method, "PUT") {
            builder = builder.body(body.to_vec());
        }
        builder
            .send()
            .map_err(|e| StoreError::Internal(e.to_string()))
    }
}

impl ObjectStore for S3ObjectStore {
    fn put_bytes(&self, key: &str, bytes: Vec<u8>) -> ObjectResult<()> {
        let resp = self.sign_and_send("PUT", key, &bytes, Some("application/octet-stream"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(StoreError::Internal(format!(
                "s3 put failed: {status} {body}"
            )));
        }
        Ok(())
    }

    fn get_bytes(&self, key: &str) -> ObjectResult<Vec<u8>> {
        let resp = self.sign_and_send("GET", key, &[], None)?;
        if resp.status().as_u16() == 404 {
            return Err(StoreError::NotFound(key.into()));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(StoreError::Internal(format!(
                "s3 get failed: {status} {body}"
            )));
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| StoreError::Internal(e.to_string()))
    }

    fn delete(&self, key: &str) -> ObjectResult<()> {
        let resp = self.sign_and_send("DELETE", key, &[], None)?;
        if resp.status().as_u16() == 404 || resp.status().is_success() {
            return Ok(());
        }
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        Err(StoreError::Internal(format!(
            "s3 delete failed: {status} {body}"
        )))
    }

    fn exists(&self, key: &str) -> ObjectResult<bool> {
        let resp = self.sign_and_send("HEAD", key, &[], None)?;
        Ok(resp.status().is_success())
    }

    fn presign_put(&self, key: &str, ttl_secs: u64) -> ObjectResult<String> {
        Ok(self.presign("PUT", key, ttl_secs))
    }

    fn presign_get(&self, key: &str, ttl_secs: u64) -> ObjectResult<String> {
        Ok(self.presign("GET", key, ttl_secs))
    }
}

impl S3ObjectStore {
    fn presign(&self, method: &str, key: &str, ttl_secs: u64) -> String {
        let amz_date = amz_date_now();
        let date_stamp = &amz_date[..8];
        let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", self.region);
        let credential = format!("{}/{}", self.access_key, credential_scope);
        let canonical_uri = format!("/{}/{}", self.bucket, key.trim_start_matches('/'));
        let host = self.host_header();
        let query = format!(
            "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential={}&X-Amz-Date={}&X-Amz-Expires={}&X-Amz-SignedHeaders=host",
            urlencoding::encode(&credential),
            amz_date,
            ttl_secs.max(1)
        );
        // 查询串需按 key 排序；上面已按字母序
        let canonical_request =
            format!("{method}\n{canonical_uri}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            hex::encode(Sha256::digest(canonical_request.as_bytes()))
        );
        let signing_key = derive_signing_key(&self.secret_key, date_stamp, &self.region, "s3");
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        format!(
            "{}{}?{}&X-Amz-Signature={}",
            self.endpoint, canonical_uri, query, signature
        )
    }
}

fn amz_date_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // YYYYMMDD'T'HHMMSS'Z' — 用 chrono 更稳，这里用简易 UTC 格式化
    let dt = chrono::DateTime::from_timestamp(secs as i64, 0)
        .unwrap_or_else(|| chrono::DateTime::UNIX_EPOCH);
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn derive_signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}
