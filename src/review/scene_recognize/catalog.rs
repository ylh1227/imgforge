//! 场景清单：本地 JSON，供云端识别约束候选。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::storage::paths::app_data_dir;

/// 单条场景定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneSpec {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// 场景目录。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneCatalog {
    #[serde(default)]
    pub scenes: Vec<SceneSpec>,
}

impl SceneCatalog {
    pub fn path() -> ReviewResult<PathBuf> {
        Ok(app_data_dir()?.join("scene_catalog.json"))
    }

    pub fn load() -> ReviewResult<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> ReviewResult<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    pub fn find_by_id(&self, id: &str) -> Option<&SceneSpec> {
        self.scenes.iter().find(|s| s.id == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&SceneSpec> {
        self.scenes.iter().find(|s| s.name == name)
    }

    /// 已知场景显示名列表（用于剥前缀）。
    pub fn names(&self) -> Vec<&str> {
        self.scenes.iter().map(|s| s.name.as_str()).collect()
    }

    pub fn validate(&self) -> ReviewResult<()> {
        if self.scenes.is_empty() {
            return Err(ReviewError::Message("场景列表为空，请先配置场景".into()));
        }
        let mut ids = std::collections::HashSet::new();
        for s in &self.scenes {
            if s.id.trim().is_empty() || s.name.trim().is_empty() {
                return Err(ReviewError::Message(
                    "场景 id / name 不能为空".into(),
                ));
            }
            if !ids.insert(s.id.as_str()) {
                return Err(ReviewError::Message(format!("重复场景 id：{}", s.id)));
            }
        }
        Ok(())
    }

    /// 从表格文本导入（首行可选表头：id,name,description；分隔符为逗号或制表符）。
    pub fn from_table_text(text: &str) -> ReviewResult<Self> {
        let mut scenes = Vec::new();
        for (line_no, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let cols = split_table_row(line);
            if cols.is_empty() {
                continue;
            }
            // skip header
            if line_no == 0 {
                let head = cols[0].to_ascii_lowercase();
                if head == "id" || head == "场景id" || head == "scene_id" {
                    continue;
                }
            }
            if cols.len() < 2 {
                return Err(ReviewError::Message(format!(
                    "第 {} 行至少需要 id 与 name 两列",
                    line_no + 1
                )));
            }
            scenes.push(SceneSpec {
                id: cols[0].trim().to_string(),
                name: cols[1].trim().to_string(),
                description: cols.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            });
        }
        let cat = Self { scenes };
        cat.validate()?;
        Ok(cat)
    }

    /// 导出为 CSV 表格（含表头）。
    pub fn to_csv_table(&self) -> String {
        let mut out = String::from("id,name,description\n");
        for s in &self.scenes {
            out.push_str(&csv_escape(&s.id));
            out.push(',');
            out.push_str(&csv_escape(&s.name));
            out.push(',');
            out.push_str(&csv_escape(&s.description));
            out.push('\n');
        }
        out
    }

    /// 从外部文件导入表格（CSV / TSV / TXT / Excel `.xlsx` `.xls`）。
    pub fn from_table_file(path: &std::path::Path) -> ReviewResult<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "xlsx" | "xls" | "xlsm" | "ods" => Self::from_excel_file(path),
            _ => {
                let raw = std::fs::read(path)?;
                let text = decode_table_bytes(&raw);
                Self::from_table_text(&text)
            }
        }
    }

    /// 合并场景：同 id 覆盖，其余追加。
    pub fn merge_with(&mut self, other: SceneCatalog) {
        for s in other.scenes {
            if let Some(existing) = self.scenes.iter_mut().find(|x| x.id == s.id) {
                *existing = s;
            } else {
                self.scenes.push(s);
            }
        }
    }

    fn from_excel_file(path: &std::path::Path) -> ReviewResult<Self> {
        use calamine::{open_workbook_auto, Reader};

        let mut workbook = open_workbook_auto(path)
            .map_err(|e| ReviewError::Message(format!("打开表格失败：{e}")))?;
        let sheet_names = workbook.sheet_names().to_vec();
        let sheet = sheet_names
            .first()
            .ok_or_else(|| ReviewError::Message("Excel 无工作表".into()))?;
        let range = workbook
            .worksheet_range(sheet)
            .map_err(|e| ReviewError::Message(format!("读取工作表失败：{e}")))?;

        let mut lines: Vec<String> = Vec::new();
        for row in range.rows() {
            let cols: Vec<String> = row.iter().map(cell_to_string).collect();
            if cols.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            lines.push(cols.join("\t"));
        }
        Self::from_table_text(&lines.join("\n"))
    }
}

fn cell_to_string(c: &calamine::Data) -> String {
    use calamine::Data;
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn decode_table_bytes(raw: &[u8]) -> String {
    // UTF-8 BOM
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&raw[3..]).into_owned();
    }
    // UTF-16 LE BOM (Excel 另存 CSV 常见)
    if raw.starts_with(&[0xFF, 0xFE]) && raw.len() >= 4 {
        let u16s: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }
    String::from_utf8_lossy(raw).into_owned()
}

fn split_table_row(line: &str) -> Vec<String> {
    if line.contains('\t') {
        return line.split('\t').map(|s| s.to_string()).collect();
    }
    // simple CSV with quotes
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                cols.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    cols.push(cur);
    cols
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let cat = SceneCatalog {
            scenes: vec![
                SceneSpec {
                    id: "night".into(),
                    name: "夜景".into(),
                    description: "低光室外".into(),
                },
                SceneSpec {
                    id: "portrait".into(),
                    name: "人像".into(),
                    description: String::new(),
                },
            ],
        };
        let text = serde_json::to_string(&cat).unwrap();
        let back: SceneCatalog = serde_json::from_str(&text).unwrap();
        assert_eq!(back.scenes.len(), 2);
        assert_eq!(back.find_by_id("night").unwrap().name, "夜景");
    }

    #[test]
    fn reject_empty() {
        assert!(SceneCatalog::default().validate().is_err());
    }

    #[test]
    fn from_csv_table() {
        let text = "id,name,description\nnight,夜景,低光\nportrait,人像,\n";
        let cat = SceneCatalog::from_table_text(text).unwrap();
        assert_eq!(cat.scenes.len(), 2);
        assert_eq!(cat.scenes[0].name, "夜景");
        let csv = cat.to_csv_table();
        assert!(csv.contains("夜景"));
    }

    #[test]
    fn from_tsv_table() {
        let text = "id\tname\tdescription\nindoor\t室内\t\n";
        let cat = SceneCatalog::from_table_text(text).unwrap();
        assert_eq!(cat.scenes[0].id, "indoor");
    }
}
