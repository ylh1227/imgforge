//! EXIF 元数据读写步骤。

use std::io::Cursor;

use crate::core::context::ImageContext;
use crate::core::error::AppResult;
use crate::core::types::MetadataPolicy;
use crate::processing::pipeline::ProcessStep;

/// 元数据处理步骤（读取或写入）。
pub struct MetadataStep {
    mode: MetadataMode,
}

enum MetadataMode {
    Read,
    Write,
}

impl MetadataStep {
    pub fn read() -> Self {
        Self {
            mode: MetadataMode::Read,
        }
    }

    pub fn write() -> Self {
        Self {
            mode: MetadataMode::Write,
        }
    }
}

impl ProcessStep for MetadataStep {
    fn name(&self) -> &'static str {
        match self.mode {
            MetadataMode::Read => "metadata_read",
            MetadataMode::Write => "metadata_write",
        }
    }

    fn execute(&self, ctx: &mut ImageContext) -> AppResult<()> {
        match self.mode {
            MetadataMode::Read => read_exif(ctx),
            MetadataMode::Write => write_exif(ctx),
        }
    }
}

fn read_exif(ctx: &mut ImageContext) -> AppResult<()> {
    if ctx.metadata_policy != MetadataPolicy::Preserve {
        return Ok(());
    }

    let bytes = match ctx.raw_bytes.as_ref() {
        Some(b) => b,
        None => return Ok(()),
    };

    let mut cursor = Cursor::new(bytes);
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) {
        // 保留原始 EXIF 字节供后续写入（简化：重新序列化字段摘要）
        let _ = exif;
        ctx.exif_bytes = extract_exif_segment(bytes);
    }
    Ok(())
}

fn write_exif(ctx: &mut ImageContext) -> AppResult<()> {
    if ctx.metadata_policy == MetadataPolicy::Strip {
        ctx.exif_bytes = None;
    }
    // JPEG EXIF 嵌入需要专门库；此处保留 exif_bytes 供后续扩展
    Ok(())
}

fn extract_exif_segment(data: &[u8]) -> Option<Vec<u8>> {
    // 查找 JPEG APP1 EXIF 段
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut offset = 2;
    while offset + 4 < data.len() {
        if data[offset] != 0xFF {
            break;
        }
        let marker = data[offset + 1];
        if marker == 0xE1 {
            let len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            if offset + 2 + len <= data.len() {
                let segment = &data[offset + 4..offset + 2 + len];
                if segment.starts_with(b"Exif\0\0") {
                    return Some(segment.to_vec());
                }
            }
        }
        if marker == 0xDA {
            break;
        }
        let seg_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 2 + seg_len;
    }
    None
}
