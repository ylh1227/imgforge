//! 单图多质量编码体积预览（后台线程）。

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use image::ImageReader;

use crate::core::types::{ImageFormat, Quality};
use crate::processing::backends::native_backend::encode_dynamic_image;
use crate::processing::quality_fit::supports_quality_target;

#[derive(Debug, Clone)]
pub struct QualitySizeRow {
  pub quality: u8,
  pub bytes: usize,
}

pub enum QualityPreviewMsg {
  Done(Vec<QualitySizeRow>),
  Failed(String),
}

pub struct QualityPreviewWorker {
  rx: Receiver<QualityPreviewMsg>,
}

impl QualityPreviewWorker {
  pub fn spawn(sample: PathBuf, format: ImageFormat) -> Self {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
      let result = compute_preview(&sample, format);
      let _ = tx.send(result);
    });
    Self { rx }
  }

  pub fn poll(&self) -> Option<QualityPreviewMsg> {
    match self.rx.try_recv() {
      Ok(msg) => Some(msg),
      Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
    }
  }
}

fn compute_preview(path: &PathBuf, format: ImageFormat) -> QualityPreviewMsg {
  if !supports_quality_target(format) {
    return QualityPreviewMsg::Failed(format!(
      "{} 格式不支持按质量估算体积",
      format.extension()
    ));
  }

  let reader = match ImageReader::open(path) {
    Ok(r) => r,
    Err(e) => return QualityPreviewMsg::Failed(e.to_string()),
  };
  let reader = match reader.with_guessed_format() {
    Ok(r) => r,
    Err(e) => return QualityPreviewMsg::Failed(e.to_string()),
  };
  let image = match reader.decode() {
    Ok(img) => img,
    Err(e) => return QualityPreviewMsg::Failed(e.to_string()),
  };

  let qualities = [50u8, 65, 75, 85, 95];
  let mut rows = Vec::new();
  for q in qualities {
    let quality = match Quality::new(q) {
      Ok(v) => v,
      Err(e) => return QualityPreviewMsg::Failed(e.to_string()),
    };
    match encode_dynamic_image(&image, format, quality) {
      Ok(bytes) => rows.push(QualitySizeRow {
        quality: q,
        bytes: bytes.len(),
      }),
      Err(e) => return QualityPreviewMsg::Failed(e.to_string()),
    }
  }

  QualityPreviewMsg::Done(rows)
}

pub fn format_bytes(bytes: usize) -> String {
  const KB: usize = 1024;
  const MB: usize = KB * 1024;
  if bytes >= MB {
    format!("{:.2} MB", bytes as f64 / MB as f64)
  } else if bytes >= KB {
    format!("{:.1} KB", bytes as f64 / KB as f64)
  } else {
    format!("{bytes} B")
  }
}
