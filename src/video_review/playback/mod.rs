//! 审片播放后端：libmpv 常驻解码 + 共享同步时钟。
//!
//! 未启用 `mpv` feature 时使用 stub（抽帧预览仍可用，便于 Windows/CI）。

#[cfg(feature = "mpv")]
mod compare_player;
#[cfg(feature = "mpv")]
mod glow_bridge;
#[cfg(feature = "mpv")]
mod mpv_session;
#[cfg(feature = "mpv")]
mod present;
#[cfg(feature = "mpv")]
mod sync_clock;

#[cfg(not(feature = "mpv"))]
mod stub;

#[cfg(feature = "mpv")]
pub use compare_player::{
    ComparePlayer, GpuPaneTexture, PaneTexture, PlaybackBackendInfo, ScopeSampleThrottle, SeekKind,
};
#[cfg(feature = "mpv")]
pub use glow_bridge::GlowBridge;
#[cfg(feature = "mpv")]
pub use mpv_session::FidelityMode;
#[cfg(feature = "mpv")]
pub use present::{PresentMode, RgbaFrame};
#[cfg(feature = "mpv")]
pub use sync_clock::SyncClock;

#[cfg(not(feature = "mpv"))]
pub use stub::*;

/// 探测系统是否能创建 libmpv 实例。
pub fn probe_libmpv() -> Result<(), String> {
    #[cfg(not(feature = "mpv"))]
    {
        return Err("mpv feature disabled".into());
    }
    #[cfg(feature = "mpv")]
    {
        use libmpv2::Mpv;
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vo", "null")?;
            init.set_option("ao", "null")?;
            init.set_option("idle", "yes")?;
            init.set_option("audio", "no")?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        drop(mpv);
        Ok(())
    }
}
