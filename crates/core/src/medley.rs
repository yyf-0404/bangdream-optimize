pub mod candidate;
pub(crate) mod disjoint_bound;
mod enumeration;
pub mod error;
pub(crate) use crate::team_prune as prune;
pub(crate) mod scoring;
pub(crate) mod seed;
pub mod team;

pub(crate) fn preparation_enabled() -> bool {
    #[cfg(all(feature = "experimental-fast-preparation", not(target_arch = "wasm32")))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        return crate::team_prune::priced::enabled()
            && *ENABLED.get_or_init(|| {
                std::env::var("BANGDREAM_OPTIMIZE_FAST_PREPARATION").as_deref() != Ok("off")
            });
    }
    #[allow(unreachable_code)]
    crate::team_prune::priced::enabled()
}

#[cfg(test)]
pub(crate) mod test_support;
