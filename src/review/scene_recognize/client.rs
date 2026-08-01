//! OpenAI 兼容多模态场景匹配客户端。

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use image::imageops::FilterType;
use image::{GenericImageView, ImageFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::scene_recognize::catalog::SceneCatalog;
use crate::review::scene_recognize::config::SceneRecognizeConfig;

/// 单图匹配结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneMatch {
    pub image_id: i64,
    /// catalog 内 id；未匹配为 None。
    pub scene_id: Option<String>,
    pub scene_name: Option<String>,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelChoice {
    scene_id: String,
    #[serde(default)]
    confidence: Option<f32>,
}

pub struct VisionSceneClient {
    config: SceneRecognizeConfig,
    api_key: String,
    http: reqwest::blocking::Client,
}

impl VisionSceneClient {
    pub fn new(config: SceneRecognizeConfig) -> ReviewResult<Self> {
        let api_key = SceneRecognizeConfig::resolve_api_key().ok_or_else(|| {
            ReviewError::Message(
                "未配置视觉 API Key：请在场景识别设置中写入钥匙串，或设置环境变量 IMGFORGE_VISION_API_KEY"
                    .into(),
            )
        })?;
        Self::validate_base_url(&config.base_url)?;
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(5)))
            .build()
            .map_err(|e| ReviewError::Message(format!("HTTP 客户端初始化失败：{e}")))?;
        Ok(Self {
            config,
            api_key,
            http,
        })
    }

    fn validate_base_url(base_url: &str) -> ReviewResult<()> {
        let u = base_url.trim();
        let ok = u.starts_with("https://")
            || u.starts_with("http://127.0.0.1")
            || u.starts_with("http://localhost");
        if !ok {
            return Err(ReviewError::Message(
                "API Base URL 必须使用 HTTPS（本地调试可用 http://127.0.0.1）".into(),
            ));
        }
        Ok(())
    }

    pub fn match_one(
        &self,
        image_id: i64,
        path: &Path,
        catalog: &SceneCatalog,
    ) -> SceneMatch {
        match self.match_one_inner(path, catalog) {
            Ok((scene_id, confidence)) => {
                if scene_id == "unknown" {
                    SceneMatch {
                        image_id,
                        scene_id: None,
                        scene_name: None,
                        confidence,
                        error: None,
                    }
                } else if let Some(spec) = catalog.find_by_id(&scene_id) {
                    SceneMatch {
                        image_id,
                        scene_id: Some(spec.id.clone()),
                        scene_name: Some(spec.name.clone()),
                        confidence,
                        error: None,
                    }
                } else {
                    SceneMatch {
                        image_id,
                        scene_id: None,
                        scene_name: None,
                        confidence: 0.0,
                        error: Some(format!("模型返回未知 scene_id：{scene_id}")),
                    }
                }
            }
            Err(e) => SceneMatch {
                image_id,
                scene_id: None,
                scene_name: None,
                confidence: 0.0,
                error: Some(e.to_string()),
            },
        }
    }

    fn match_one_inner(
        &self,
        path: &Path,
        catalog: &SceneCatalog,
    ) -> ReviewResult<(String, f32)> {
        let b64 = encode_image_jpeg_base64(path, self.config.max_edge)?;
        let scenes_json = serde_json::to_string_pretty(&catalog.scenes)?;
        let system = "You are a scene classifier for imaging QA. \
Respond with ONLY a JSON object: {\"scene_id\":\"...\",\"confidence\":0.0-1.0}. \
scene_id MUST be one of the provided catalog ids, or \"unknown\" if none fits.";
        let user_text = format!(
            "Pick the best matching scene for this photo from the catalog.\n\nCatalog:\n{scenes_json}"
        );

        let mut body = json!({
            "model": self.config.model,
            "temperature": 0,
            "max_tokens": 96,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": user_text },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/jpeg;base64,{b64}")
                            }
                        }
                    ]
                }
            ]
        });
        // DashScope Thinking / 混合思考模型：场景分类关闭思考可明显加速。
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "enable_thinking".into(),
                json!(self.config.enable_thinking),
            );
            if self.config.thinking_budget > 0 {
                obj.insert(
                    "thinking_budget".into(),
                    json!(self.config.thinking_budget),
                );
            }
        }

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let mut last_err = None;
        for attempt in 0..2 {
            match self.post_completion(&url, &body) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    last_err = Some(e);
                    if attempt == 0 {
                        std::thread::sleep(Duration::from_millis(400));
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| ReviewError::Message("识别失败".into())))
    }

    fn post_completion(&self, url: &str, body: &Value) -> ReviewResult<(String, f32)> {
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .map_err(|e| ReviewError::Message(format!("视觉 API 请求失败：{e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| ReviewError::Message(format!("读取 API 响应失败：{e}")))?;
        if !status.is_success() {
            let snippet: String = text.chars().take(240).collect();
            let snippet = crate::review::scene_recognize::secret::redact_secrets(&snippet);
            return Err(ReviewError::Message(format!(
                "视觉 API HTTP {status}：{snippet}"
            )));
        }

        let root: Value = serde_json::from_str(&text)
            .map_err(|e| ReviewError::Message(format!("API JSON 无效：{e}")))?;
        let content = root
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ReviewError::Message("API 响应缺少 message.content".into()))?;

        let choice: ModelChoice = serde_json::from_str(content).or_else(|_| {
            // 容错：从文本中抽 JSON 对象
            extract_json_object(content)
                .ok_or_else(|| ReviewError::Message(format!("无法解析模型输出：{content}")))
                .and_then(|s| {
                    serde_json::from_str(s)
                        .map_err(|e| ReviewError::Message(format!("模型 JSON 解析失败：{e}")))
                })
        })?;

        let conf = choice.confidence.unwrap_or(0.5).clamp(0.0, 1.0);
        Ok((choice.scene_id.trim().to_string(), conf))
    }
}

fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

fn encode_image_jpeg_base64(path: &Path, max_edge: u32) -> ReviewResult<String> {
    let img = image::open(path).map_err(|source| ReviewError::ImageDecode {
        path: path.to_path_buf(),
        source,
    })?;
    let resized = {
        let (w, h) = img.dimensions();
        let edge = max_edge.max(64);
        if w <= edge && h <= edge {
            img
        } else {
            img.resize(edge, edge, FilterType::Triangle)
        }
    };
    let rgb = resized.to_rgb8();
    let mut buf = Vec::new();
    rgb.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
        .map_err(|e| ReviewError::Message(format!("JPEG 编码失败：{e}")))?;
    Ok(b64_encode(&buf))
}

fn b64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let mut n = (chunk[0] as u32) << 16;
        if chunk.len() > 1 {
            n |= (chunk[1] as u32) << 8;
        }
        if chunk.len() > 2 {
            n |= chunk[2] as u32;
        }
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_hello() {
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn extract_json() {
        let s = "here is {\"scene_id\":\"night\",\"confidence\":0.9} done";
        let j = extract_json_object(s).unwrap();
        let c: ModelChoice = serde_json::from_str(j).unwrap();
        assert_eq!(c.scene_id, "night");
    }
}
