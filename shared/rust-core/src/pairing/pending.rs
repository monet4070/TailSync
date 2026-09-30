//! Durable, deliberately **non-authoritative** record of pairings that both
//! users confirmed but that are not yet known to be durable on both devices
//! (finding S3-P1-2).
//!
//! The failure this exists for: the pairing session persisted trust as soon as
//! both users had confirmed, before the peer had acknowledged its own write. If
//! the link dropped or the session timed out in between, one side held a
//! durable trust record for a device the other side had never accepted. Nothing
//! recorded that the record was half-formed, so nothing could reconcile it.
//!
//! Two invariants keep the fix honest:
//!
//! 1. A record here is never consulted for admission. `peer_is_allowed`,
//!    the connection pool and the peer directory all read
//!    `Settings::trusted_peer_keys`; a device whose pairing is still pending
//!    stays out of that map and is therefore reported as unpaired everywhere,
//!    including both UIs, without any of them having to learn a new state.
//! 2. The record lives in a sidecar file rather than as a new `Settings` field.
//!    `config-v2.json` is parsed with `deny_unknown_fields`, so an older build
//!    that met a new key there would fail to load *all* settings. An older build
//!    does not know this file at all, and its behaviour is then exactly the safe
//!    one: with no key in `trusted_peer_keys` it treats the device as unpaired.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Sidecar next to `config-v2.json` in the data directory.
const STORE_FILE: &str = "pairing-pending.json";
const STORE_FORMAT_VERSION: u32 = 1;

/// One half-confirmed pairing: the local user confirmed the verification code
/// and the local side has written this record, but the peer has not yet been
/// observed to have persisted its own side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrustRecord {
    pub hostname: String,
    /// base64 STANDARD of the peer's pinned Noise static public key.
    pub public_key: String,
    pub interface: String,
    pub address: String,
    /// Unix seconds at which this side recorded the confirmation.
    pub recorded_at: u64,
    /// How many times reconciliation was attempted. Kept so an unreachable peer
    /// is visible rather than silently retried forever; nothing expires on the
    /// count, because a peer that already persisted and lost the
    /// acknowledgement must still be recoverable later.
    #[serde(default)]
    pub reconciliations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default = "format_version")]
    format_version: u32,
    #[serde(default)]
    records: Vec<PendingTrustRecord>,
}

fn format_version() -> u32 {
    STORE_FORMAT_VERSION
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            format_version: STORE_FORMAT_VERSION,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingTrustStore {
    /// `None` keeps the store in memory only. Tests use it so a pairing test
    /// never writes into the machine's real data directory; production always
    /// has a path, because a pending record that does not survive a restart is
    /// exactly the state this module exists to preserve.
    path: Option<PathBuf>,
    records: Vec<PendingTrustRecord>,
}

impl PendingTrustStore {
    /// Load the store from the default data directory.
    pub fn load() -> Self {
        Self::load_from_path(&store_path())
    }

    /// A store that keeps records for this process only. Nothing about a pending
    /// record needs to be authoritative, so a caller that must not touch the
    /// filesystem (tests, in-memory managers) loses only durability.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            records: Vec::new(),
        }
    }

    /// Load the store, treating a missing, unreadable or unparsable file as
    /// empty. That direction is the safe one: discarding a pending record can
    /// only cause a re-pairing, while adopting anything from an unreadable file
    /// could turn a corrupted byte into trust.
    pub fn load_from_path(path: &Path) -> Self {
        let records = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<StoreFile>(&text) {
                Ok(file) if file.format_version == STORE_FORMAT_VERSION => file.records,
                Ok(file) => {
                    log::warn!(
                        "Ignoring pending pairing store {} with unsupported format version {}",
                        path.display(),
                        file.format_version
                    );
                    Vec::new()
                }
                Err(error) => {
                    log::warn!(
                        "Ignoring unreadable pending pairing store {}: {error}",
                        path.display()
                    );
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                log::warn!(
                    "Could not read pending pairing store {}: {error}",
                    path.display()
                );
                Vec::new()
            }
        };
        Self {
            path: Some(path.to_path_buf()),
            records,
        }
    }

    pub fn records(&self) -> &[PendingTrustRecord] {
        &self.records
    }

    pub fn get(&self, hostname: &str) -> Option<&PendingTrustRecord> {
        self.records
            .iter()
            .find(|record| record.hostname == hostname)
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Record (or refresh) a pending pairing. The record is written before the
    /// pairing session tells the peer that it persisted, so a crash between the
    /// two leaves a recoverable note rather than a half-claimed pairing.
    pub fn upsert(&mut self, record: PendingTrustRecord) -> std::io::Result<()> {
        self.records
            .retain(|known| known.hostname != record.hostname);
        self.records.push(record);
        self.save()
    }

    /// Drop the pending record. Called once the pairing became durable on both
    /// sides, or when the user forgets the device.
    pub fn remove(&mut self, hostname: &str) -> std::io::Result<()> {
        self.records.retain(|known| known.hostname != hostname);
        self.save()
    }

    pub fn note_reconciliation_attempt(&mut self, hostname: &str) -> std::io::Result<()> {
        if let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.hostname == hostname)
        {
            record.reconciliations = record.reconciliations.saturating_add(1);
        }
        self.save()
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = StoreFile {
            format_version: STORE_FORMAT_VERSION,
            records: self.records.clone(),
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        crate::private_fs::write_private_file(path, json.as_bytes())
    }
}

pub fn store_path() -> PathBuf {
    crate::db::get_data_dir().join(STORE_FILE)
}
