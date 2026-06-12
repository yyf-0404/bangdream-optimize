#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Timer {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    start_ms: f64,
}

impl Timer {
    pub(crate) fn start() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
            #[cfg(target_arch = "wasm32")]
            start_ms: performance_now(),
        }
    }

    pub(crate) fn elapsed_ms(&self) -> f64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_secs_f64() * 1000.0
        }

        #[cfg(target_arch = "wasm32")]
        {
            performance_now() - self.start_ms
        }
    }
}

pub(crate) fn optional_elapsed_ms(start: Option<Timer>) -> f64 {
    start.map(|timer| timer.elapsed_ms()).unwrap_or(0.0)
}
