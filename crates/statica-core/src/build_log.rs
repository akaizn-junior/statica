//! Optional build-step logging (plain stderr; CLI adds styled summary).

use std::time::Instant;

/// When enabled, emits plain step lines to stderr during [`crate::build::build`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildLog {
    pub enabled: bool,
}

impl BuildLog {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn step(&self, msg: impl AsRef<str>) {
        if self.enabled {
            eprintln!("  {}", msg.as_ref());
        }
    }

    pub fn timed<F>(&self, label: &str, f: F) -> u128
    where
        F: FnOnce(),
    {
        let t = Instant::now();
        f();
        let ms = t.elapsed().as_millis();
        if self.enabled {
            eprintln!("  {label} ({ms}ms)");
        }
        ms
    }
}
