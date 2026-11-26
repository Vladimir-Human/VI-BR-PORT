//! One tracing setup for every binary.
//!
//! Filter with `VICTORY_LOG` (falls back to `RUST_LOG`), e.g.
//! `VICTORY_LOG=victory_loader=trace,info`. Debug and trace output exist
//! only in builds with the `diagnostics` feature; a plain release build
//! logs info and above whatever the filter asks for.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The most verbose level this build can log.
const CAP: LevelFilter = if cfg!(feature = "diagnostics") {
    LevelFilter::TRACE
} else {
    LevelFilter::INFO
};

pub fn init(default_level: &str) {
    let filter = std::env::var("VICTORY_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| EnvFilter::new(default_level));
    let _ = tracing_subscriber::registry()
        .with(CAP)
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_timer(LocalTime)
                .with_writer(std::io::stderr),
        )
        .try_init();
}

/// Each line's stamp in the PC's own time, day first (26/11/2026 19:39:03), as it is read where
/// the fork is played; not the default UTC (2026-11-26T22:39:03.991169Z).
struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        #[cfg(windows)]
        {
            // SAFETY: GetLocalTime only writes the SYSTEMTIME it is given.
            let t = unsafe {
                let mut t = std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
                windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
                t
            };
            write!(w, "{:02}/{:02}/{:04} {:02}:{:02}:{:02}", t.wDay, t.wMonth, t.wYear, t.wHour, t.wMinute, t.wSecond)
        }
        #[cfg(not(windows))]
        {
            tracing_subscriber::fmt::time::SystemTime.format_time(w)
        }
    }
}
