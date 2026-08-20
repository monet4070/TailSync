// Echo-suppression shadow filter (T322 extraction from sync.rs).
//
// Clipboard backends can emit several events for one programmatic write;
// every one of those echoes must be suppressed for a short TTL. Entries are
// intentionally sticky for the full TTL — a user copying identical content
// during the window is suppressed as well.

use std::collections::HashMap;
use std::time::{Duration, Instant};

const SHADOW_FILTER_TTL: Duration = Duration::from_secs(30);
pub(crate) const SHADOW_FILTER_MAX_ENTRIES: usize = 1024;

struct ShadowEntry {
    expires_at: Instant,
}

pub(crate) struct ShadowFilter {
    entries: HashMap<String, ShadowEntry>,
}

impl ShadowFilter {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, hash: String) {
        let now = Instant::now();
        self.prune(now);
        if let Some(entry) = self.entries.get_mut(&hash) {
            entry.expires_at = now + SHADOW_FILTER_TTL;
            return;
        }
        if self.entries.len() >= SHADOW_FILTER_MAX_ENTRIES {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(hash, _)| hash.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(
            hash,
            ShadowEntry {
                expires_at: now + SHADOW_FILTER_TTL,
            },
        );
    }

    /// Shadow entries intentionally remain sticky for the full TTL. Clipboard
    /// backends can emit several events for one programmatic write, and every
    /// one of those echoes must be suppressed. The accepted trade-off is that
    /// a user copying identical content during the TTL is suppressed as well.
    pub(crate) fn contains(&mut self, hash: &str) -> bool {
        self.prune(Instant::now());
        self.entries.contains_key(hash)
    }

    pub(crate) fn remove(&mut self, hash: &str) -> bool {
        self.entries.remove(hash).is_some()
    }

    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, entry| entry.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_filter_stays_sticky_and_bounded() {
        let mut filter = ShadowFilter::new();
        filter.insert("same".into());
        filter.insert("same".into());
        assert!(filter.contains("same"));
        assert!(filter.contains("same"));
        assert_eq!(filter.entries.len(), 1);

        for index in 0..(SHADOW_FILTER_MAX_ENTRIES + 20) {
            filter.insert(format!("hash-{index}"));
        }
        assert_eq!(filter.entries.len(), SHADOW_FILTER_MAX_ENTRIES);
    }

    #[test]
    fn shadow_filter_remove_rolls_back_and_expired_entries_miss() {
        let mut filter = ShadowFilter::new();
        filter.insert("rollback".into());
        assert!(filter.remove("rollback"));
        assert!(!filter.contains("rollback"));

        filter.insert("expired".into());
        filter.entries.get_mut("expired").unwrap().expires_at = Instant::now();
        assert!(!filter.contains("expired"));
    }
}
