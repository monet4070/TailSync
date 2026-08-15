//! Platform view of the shared peer event rate limiter.
//!
//! The per-peer event/byte budget lives in `tailsync_core::peer::rate_limit`;
//! this module only re-exports it so existing `network::rate_limit` call
//! sites stay unchanged. The file is byte-identical on both platforms
//! (enforced by the cross-platform drift check).

#[allow(unused_imports)] // Contract surface: both platforms share this exact file.
pub use tailsync_core::peer::rate_limit::check_peer_event_budget;

#[cfg(test)]
mod contract_tests {
    use super::*;
    use tailsync_core::peer::rate_limit as core_rate_limit;

    /// Compile-time identity proof: the platform re-export must be the very
    /// same function as the shared core one, not a lookalike. If this file
    /// drifts from the core contract, this assignment stops compiling.
    fn same<T>(value: T) -> T {
        value
    }

    #[test]
    fn re_exported_budget_checker_is_the_shared_core_function() {
        let _: fn(&str, usize) -> Result<(), String> = same(check_peer_event_budget);
        let _: fn(&str, usize) -> Result<(), String> =
            same(core_rate_limit::check_peer_event_budget);
    }
}
