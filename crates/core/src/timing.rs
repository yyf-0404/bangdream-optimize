//! Cross-platform monotonic timing for code shared with the browser WASM build.
//!
//! Do not call `std::time::Instant::now()` directly from shared calculation paths:
//! it can compile for `wasm32-unknown-unknown` and then panic when executed in a
//! browser. Use `Timer` so native builds use `Instant` and WASM uses
//! `performance.now()`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

/// Monotonic clock for calculation diagnostics in native and browser workers.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy)]
pub struct Instant {
    start_ms: f64,
}

#[cfg(target_arch = "wasm32")]
impl Instant {
    pub fn now() -> Self {
        Self { start_ms: performance_now() }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(((performance_now() - self.start_ms) / 1000.0).max(0.0))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Timer {
    start: Instant,
}

impl Timer {
    pub(crate) fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub(crate) fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

pub(crate) fn optional_elapsed_ms(start: Option<Timer>) -> f64 {
    start.map(|timer| timer.elapsed_ms()).unwrap_or(0.0)
}
