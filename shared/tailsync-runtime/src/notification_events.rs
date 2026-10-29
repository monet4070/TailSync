//! Bounded receive-event feed. History mutations and local copies never enter it.

use crate::contracts::RuntimeNotification;
use std::collections::VecDeque;
use tailsync_core::notifications::ClipboardArrival;

pub const MAX_RUNTIME_NOTIFICATIONS: usize = 256;

#[derive(Default)]
pub struct RuntimeNotificationBuffer {
    next_id: u64,
    dropped: u64,
    entries: VecDeque<RuntimeNotification>,
}

impl RuntimeNotificationBuffer {
    pub fn push(&mut self, level: &str, message: String) -> u64 {
        self.append(level, message, None, None)
    }

    pub fn push_system(
        &mut self,
        level: &str,
        notification: crate::notification_text::SystemNotification<'_>,
        language: &str,
    ) -> u64 {
        self.append(
            level,
            notification.body(language),
            Some(notification.code().into()),
            None,
        )
    }

    pub fn push_arrival(&mut self, mut arrival: ClipboardArrival) -> Option<u64> {
        if self.entries.iter().any(|event| {
            event.arrival.as_ref().is_some_and(|existing| {
                existing.source_id == arrival.source_id && existing.event_id == arrival.event_id
            })
        }) {
            return None;
        }
        if let Some(previous) = self
            .entries
            .iter()
            .rev()
            .filter_map(|event| event.arrival.as_ref())
            .find(|previous| previous.source_id == arrival.source_id)
        {
            arrival.count += previous.count;
            if arrival.kind != previous.kind {
                arrival.kind = "content".into();
            }
        }
        Some(self.append("arrival", String::new(), None, Some(arrival)))
    }

    fn append(
        &mut self,
        level: &str,
        message: String,
        code: Option<String>,
        arrival: Option<ClipboardArrival>,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.entries.push_back(RuntimeNotification {
            id,
            level: level.into(),
            message,
            code,
            arrival,
        });
        if self.entries.len() > MAX_RUNTIME_NOTIFICATIONS {
            self.entries.pop_front();
            self.dropped += 1;
        }
        id
    }

    pub fn since(&self, id: u64) -> Vec<RuntimeNotification> {
        self.entries
            .iter()
            .filter(|event| event.id > id)
            .cloned()
            .collect()
    }

    pub fn earliest_available_id(&self) -> u64 {
        self.entries
            .front()
            .map_or(self.next_id + 1, |event| event.id)
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(event: &str) -> ClipboardArrival {
        ClipboardArrival {
            source_id: "authenticated-peer".into(),
            source_name: "MacBook".into(),
            event_id: event.into(),
            kind: "text".into(),
            count: 1,
            entry_id: Some(7),
        }
    }

    #[test]
    fn receive_events_survive_local_history_changes_and_reused_history_ids() {
        let mut buffer = RuntimeNotificationBuffer::default();
        buffer.push_arrival(arrival("remote-1"));
        // A local copy changes history, not this feed. A later remote copy may reuse the same history row.
        buffer.push_arrival(arrival("remote-2"));
        buffer.push_arrival(arrival("remote-3"));
        let events = buffer.since(0);
        assert_eq!(events.len(), 3);
        assert_eq!(events.last().unwrap().arrival.as_ref().unwrap().count, 3);
        assert!(events
            .iter()
            .all(|event| event.arrival.as_ref().unwrap().entry_id == Some(7)));
        assert!(buffer.push_arrival(arrival("remote-2")).is_none());
        assert_eq!(buffer.since(0).len(), 3);
    }

    #[test]
    fn file_batch_counts_once_and_authenticated_sources_do_not_merge_by_name() {
        let mut buffer = RuntimeNotificationBuffer::default();
        let mut files = arrival("files:batch-1");
        files.kind = "file".into();
        files.count = 10;
        buffer.push_arrival(files.clone());
        assert!(buffer.push_arrival(files).is_none());
        let mut next = arrival("files:batch-2");
        next.kind = "file".into();
        next.count = 2;
        buffer.push_arrival(next);
        let mut other = arrival("files:batch-2");
        other.source_id = "different-authenticated-peer".into();
        buffer.push_arrival(other);
        let events = buffer.since(0);
        assert_eq!(events[1].arrival.as_ref().unwrap().count, 12);
        assert_eq!(events[2].arrival.as_ref().unwrap().count, 1);
        buffer.push_arrival(arrival("text-3"));
        let events = buffer.since(0);
        let mixed = events.last().unwrap().arrival.as_ref().unwrap();
        assert_eq!((mixed.count, mixed.kind.as_str()), (13, "content"));
    }

    #[test]
    fn overflow_is_explicit_and_errors_remain_separate_events() {
        let mut buffer = RuntimeNotificationBuffer::default();
        buffer.push("error", "diagnostic".into());
        for index in 0..MAX_RUNTIME_NOTIFICATIONS {
            buffer.push_arrival(arrival(&index.to_string()));
        }
        assert_eq!(buffer.since(0).len(), MAX_RUNTIME_NOTIFICATIONS);
        assert_eq!(buffer.earliest_available_id(), 2);
        assert_eq!(buffer.dropped_total(), 1);
    }
}
