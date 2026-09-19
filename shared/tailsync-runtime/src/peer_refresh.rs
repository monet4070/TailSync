/// A refresh completion is valid only for the mode and generation a caller
/// requested. Keeping this predicate in the shared runtime prevents platform
/// refresh loops from treating a stale mode's completion as the current one.
pub fn completed_for_mode(
    generation: u64,
    baseline: u64,
    completed_mode: &str,
    requested_mode: &str,
) -> bool {
    generation > baseline && completed_mode == requested_mode
}

#[cfg(test)]
mod tests {
    use super::completed_for_mode;

    #[test]
    fn stale_generation_or_mode_cannot_complete_a_refresh_waiter() {
        assert!(!completed_for_mode(3, 3, "auto", "auto"));
        assert!(!completed_for_mode(4, 3, "lan_only", "auto"));
        assert!(completed_for_mode(4, 3, "auto", "auto"));
    }
}
mod coordinator;
pub use coordinator::*;
