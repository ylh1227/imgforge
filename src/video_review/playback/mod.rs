//! 审片播放后端：libmpv 常驻解码 + 共享同步时钟。

mod compare_player;
mod glow_bridge;
mod mpv_session;
mod present;
mod sync_clock;

pub use compare_player::{
    ComparePlayer, GpuPaneTexture, PaneTexture, PlaybackBackendInfo, ScopeSampleThrottle, SeekKind,
};
pub use glow_bridge::GlowBridge;
pub use mpv_session::FidelityMode;
pub use present::{PresentMode, RgbaFrame};
pub use sync_clock::SyncClock;

/// 探测系统是否能创建 libmpv 实例。
pub fn probe_libmpv() -> Result<(), String> {
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
