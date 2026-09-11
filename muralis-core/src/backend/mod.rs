pub mod hyprpaper;
pub mod monitor;
pub mod swww;

use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::error::Result;
use crate::models::BackendType;

#[async_trait]
pub trait WallpaperBackend: Send + Sync {
    async fn set_wallpaper(&self, path: &Path, monitor: &str) -> Result<()>;
    async fn set_wallpaper_all(&self, path: &Path) -> Result<()>;

    /// Probe whether the backend's own daemon is up and accepting requests.
    /// Every backend here drives a separate long-running process (awww-daemon,
    /// hyprpaper) that may not have bound its socket yet at login.
    async fn is_ready(&self) -> Result<()>;

    fn name(&self) -> &str;
}

/// How long to keep probing a backend before giving up on it.
///
/// Exists because muralis and the backend daemon are typically launched
/// together by the compositor with no sequencing: muralis can win the race and,
/// without waiting, lose its only startup apply of the session.
#[derive(Debug, Clone)]
pub struct ReadinessPolicy {
    /// Maximum number of probes, including the immediate first one.
    pub attempts: u32,
    /// Delay before the second probe; doubles each attempt thereafter.
    pub initial_delay: Duration,
    /// Ceiling on the doubling.
    pub max_delay: Duration,
}

impl Default for ReadinessPolicy {
    /// ~11s of patience across 10 probes — long enough for a cold login,
    /// short enough that a genuinely absent backend is reported promptly.
    fn default() -> Self {
        Self {
            attempts: 10,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }
}

impl ReadinessPolicy {
    /// Delay before probe `attempt` (0-based). The first probe is immediate.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        match attempt.checked_sub(1) {
            None => Duration::ZERO,
            Some(shift) => self
                .initial_delay
                .checked_mul(1u32.checked_shl(shift).unwrap_or(u32::MAX))
                .unwrap_or(self.max_delay)
                .min(self.max_delay),
        }
    }
}

/// Probe `backend` until it reports ready, backing off per `policy`.
/// Returns the backend's own last error if it never came up.
pub async fn wait_until_ready(
    backend: &dyn WallpaperBackend,
    policy: &ReadinessPolicy,
) -> Result<()> {
    let mut last_err = None;

    for attempt in 0..policy.attempts.max(1) {
        let delay = policy.delay_for(attempt);
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }

        match backend.is_ready().await {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        crate::error::MuralisError::Backend(format!("{} never became ready", backend.name()))
    }))
}

pub fn create_backend(config: &Config) -> Box<dyn WallpaperBackend> {
    match config.general.backend {
        BackendType::Hyprpaper => Box::new(hyprpaper::HyprpaperBackend::new()),
        BackendType::Swww => Box::new(swww::SwwwBackend::new(config.display.transition.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MuralisError;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// Backend that reports ready only from the `ready_at`-th probe onward.
    /// `ready_at = 0` means never ready.
    struct ProbeBackend {
        ready_at: u32,
        probes: AtomicU32,
    }

    impl ProbeBackend {
        fn ready_at(ready_at: u32) -> Self {
            Self {
                ready_at,
                probes: AtomicU32::new(0),
            }
        }

        fn probes(&self) -> u32 {
            self.probes.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WallpaperBackend for ProbeBackend {
        async fn set_wallpaper(&self, _path: &Path, _monitor: &str) -> Result<()> {
            Ok(())
        }

        async fn set_wallpaper_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        async fn is_ready(&self) -> Result<()> {
            let probe = self.probes.fetch_add(1, Ordering::SeqCst) + 1;
            if self.ready_at != 0 && probe >= self.ready_at {
                Ok(())
            } else {
                Err(MuralisError::Backend("daemon not listening".into()))
            }
        }

        fn name(&self) -> &str {
            "probe"
        }
    }

    /// Policy with the waiting taken out, so tests assert probe counts, not timing.
    fn no_wait(attempts: u32) -> ReadinessPolicy {
        ReadinessPolicy {
            attempts,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    #[tokio::test]
    async fn ready_backend_is_probed_once() {
        let backend = ProbeBackend::ready_at(1);
        assert!(wait_until_ready(&backend, &no_wait(5)).await.is_ok());
        assert_eq!(backend.probes(), 1);
    }

    #[tokio::test]
    async fn backend_coming_up_late_is_still_caught() {
        let backend = ProbeBackend::ready_at(3);
        assert!(wait_until_ready(&backend, &no_wait(5)).await.is_ok());
        assert_eq!(backend.probes(), 3);
    }

    #[tokio::test]
    async fn absent_backend_gives_up_and_reports_its_own_error() {
        let backend = ProbeBackend::ready_at(0);
        let err = wait_until_ready(&backend, &no_wait(4)).await.unwrap_err();
        assert!(
            err.to_string().contains("daemon not listening"),
            "expected the backend error to survive, got: {err}"
        );
        assert_eq!(backend.probes(), 4);
    }

    #[test]
    fn backoff_doubles_up_to_the_ceiling() {
        let policy = ReadinessPolicy {
            attempts: 10,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        };

        // first probe is immediate, then 100ms doubling to the 2s ceiling
        let delays: Vec<u64> = (0..7)
            .map(|a| policy.delay_for(a).as_millis() as u64)
            .collect();
        assert_eq!(delays, vec![0, 100, 200, 400, 800, 1600, 2000]);

        // no overflow panic once the shift runs past u32
        assert_eq!(policy.delay_for(64), Duration::from_secs(2));
    }
}
