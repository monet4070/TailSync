//! Typed outcomes for native clipboard effects (ADR-004 K2).
//!
//! The distinction that matters to every caller is not success versus failure but
//! **whether the platform may already have changed**: only [`NativePublish::NoEffect`]
//! is safe to retry, and only [`NativePublish::Published`] may be reported to the
//! user as a completed copy. A plain `Err(String)` cannot express that, which is how
//! the previous implementation turned "we do not know" into "nothing happened".

use crate::protocol::native_clipboard::{PublishReceipt, PublishState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativePublish {
    /// Provably nothing was written: the helper never started, or it reported no
    /// effect before touching the pasteboard. Safe to retry.
    NoEffect { detail: String },
    /// The write completed and the receipt is authoritative.
    Published(PublishReceipt),
    /// Part of the write happened; the receipt describes the confirmed set.
    Partial(PublishReceipt),
    /// The platform declined because a newer copy won the race, before writing.
    Superseded(PublishReceipt),
    /// A side effect may have happened and the receipt was lost. Never retried
    /// automatically, and never reported as a success.
    Unknown { detail: String },
}

impl NativePublish {
    /// The error text a simple `Result<(), String>` seam should carry, so the caller
    /// can tell a provable no-effect from an unknown outcome.
    pub fn as_write_error(&self) -> String {
        match self {
            Self::NoEffect { detail } => {
                format!("{}{detail}", crate::protocol::NO_EFFECT_MARKER)
            }
            Self::Unknown { detail } => format!("publish outcome unknown: {detail}"),
            Self::Partial(_) => "clipboard formats were only partially published".to_string(),
            Self::Superseded(_) => "clipboard publish was superseded".to_string(),
            Self::Published(_) => String::new(),
        }
    }

    /// The receipt, when the platform produced one.
    pub fn receipt(&self) -> Option<&PublishReceipt> {
        match self {
            Self::Published(receipt) | Self::Partial(receipt) | Self::Superseded(receipt) => {
                Some(receipt)
            }
            Self::NoEffect { .. } | Self::Unknown { .. } => None,
        }
    }

    pub fn state(&self) -> Option<PublishState> {
        self.receipt().map(|receipt| receipt.state)
    }

    /// Whether the clipboard was changed, in whole or in part.
    pub fn changed_the_clipboard(&self) -> bool {
        matches!(self, Self::Published(_) | Self::Partial(_))
    }

    /// Whether the caller may retry automatically. Only a definite no-effect may be
    /// retried; everything else risks a duplicate system write.
    pub fn may_retry(&self) -> bool {
        matches!(self, Self::NoEffect { .. })
    }

    /// Whether the user must be told that the result is not a clean success.
    pub fn needs_a_visible_result(&self) -> bool {
        matches!(self, Self::Partial(_) | Self::Unknown { .. })
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::NoEffect { detail } | Self::Unknown { detail } => Some(detail),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(state: PublishState) -> PublishReceipt {
        PublishReceipt {
            state,
            revision: 3,
            token: [1; 16],
        }
    }

    /// Only a definite no-effect may be retried, and only a published write may be
    /// reported as a completed copy.
    #[test]
    fn retry_and_success_are_not_the_same_question() {
        let no_effect = NativePublish::NoEffect {
            detail: "helper missing".into(),
        };
        assert!(no_effect.may_retry());
        assert!(!no_effect.changed_the_clipboard());
        assert!(!no_effect.needs_a_visible_result());

        let unknown = NativePublish::Unknown {
            detail: "deadline expired".into(),
        };
        assert!(
            !unknown.may_retry(),
            "an unknown outcome must never be retried"
        );
        assert!(!unknown.changed_the_clipboard());
        assert!(unknown.needs_a_visible_result());

        let partial = NativePublish::Partial(receipt(PublishState::Partial));
        assert!(!partial.may_retry());
        assert!(partial.changed_the_clipboard());
        assert!(partial.needs_a_visible_result());

        let published = NativePublish::Published(receipt(PublishState::Published));
        assert!(!published.needs_a_visible_result());
        assert!(published.changed_the_clipboard());

        // A superseded attempt never wrote, but retrying it would re-publish an
        // event the platform already declined.
        let superseded = NativePublish::Superseded(receipt(PublishState::Superseded));
        assert!(!superseded.may_retry());
        assert!(!superseded.changed_the_clipboard());
    }
}
