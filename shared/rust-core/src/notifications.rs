//! Metadata emitted only after a remote receive has committed to history.

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct ClipboardArrival {
    /// Authenticated device identity, independent of the display name.
    pub source_id: String,
    pub source_name: String,
    /// Reliable message id or file batch id; never a history-list watermark.
    pub event_id: String,
    pub kind: String,
    pub count: usize,
    pub entry_id: Option<i64>,
}
