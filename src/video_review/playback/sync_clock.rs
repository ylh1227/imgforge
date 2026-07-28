//! 多路共享主时钟：播放时按墙钟推进全局时间。

use std::time::Instant;

#[derive(Debug, Clone)]
pub struct SyncClock {
    playing: bool,
    rate: f64,
    origin_instant: Instant,
    origin_ms: u64,
}

impl Default for SyncClock {
    fn default() -> Self {
        Self {
            playing: false,
            rate: 1.0,
            origin_instant: Instant::now(),
            origin_ms: 0,
        }
    }
}

impl SyncClock {
    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn set_rate(&mut self, rate: f64) {
        let now = self.now_ms();
        self.rate = rate.clamp(0.25, 4.0);
        if self.playing {
            self.origin_ms = now;
            self.origin_instant = Instant::now();
        }
    }

    /// 当前全局时间（毫秒）。
    pub fn now_ms(&self) -> u64 {
        if !self.playing {
            return self.origin_ms;
        }
        let elapsed = self.origin_instant.elapsed().as_secs_f64() * self.rate;
        let delta = (elapsed * 1000.0).round() as i64;
        (self.origin_ms as i64 + delta).max(0) as u64
    }

    pub fn pause_at(&mut self, ms: u64) {
        self.origin_ms = ms;
        self.origin_instant = Instant::now();
        self.playing = false;
    }

    pub fn play_from(&mut self, ms: u64) {
        self.origin_ms = ms;
        self.origin_instant = Instant::now();
        self.playing = true;
    }

    pub fn toggle(&mut self, ms: u64) {
        if self.playing {
            self.pause_at(ms);
        } else {
            self.play_from(ms);
        }
    }

    /// scrub / 手动 seek 时对齐原点（保持播放状态不变）。
    pub fn seek_origin(&mut self, ms: u64) {
        self.origin_ms = ms;
        self.origin_instant = Instant::now();
    }

    /// 由主路 time-pos 校准（播放中）。
    pub fn sync_from_master(&mut self, master_ms: u64) {
        if self.playing {
            self.origin_ms = master_ms;
            self.origin_instant = Instant::now();
        }
    }
}
