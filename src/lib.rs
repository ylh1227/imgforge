//! imgforge 核心库：图像处理流水线、调度与配置。

pub mod config;
pub mod core;
pub mod io;
pub mod job;
pub mod mobile;
pub mod process_util;
pub mod processing;
pub mod jira;
pub mod remote;
pub mod scheduler;
pub mod ui;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "gui")]
pub mod gui;

#[cfg(feature = "review")]
pub mod review;

#[cfg(feature = "video-review")]
pub mod video_review;

#[cfg(feature = "data-extract")]
pub mod data_extract;
