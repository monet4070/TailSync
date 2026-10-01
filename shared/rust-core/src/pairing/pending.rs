//! Durable, deliberately **non-authoritative** record of pairings that both
//! users confirmed but whose local trust has not yet been committed
//! (finding S3-P1-2).
//!
//! The failure this exists for: the pairing session persisted trust as soon as
//! both users had confirmed, before the peer had acknowledged its own write. If
//! the link dropped or the session timed out in between, one side held a
//! durable trust record while the other side was still pending. Nothing
//! recorded that the record was half-formed, so nothing could reconcile it.
//!
//! Two invariants keep the fix honest:
//!
//! 1. A record here is never consulted for admission. `peer_is_allowed`,
//!    the connection pool and the peer directory all read
//!    `Settings::trusted_peer_keys`; a device whose pairing is still pending
//!    stays out of that map. UIs may display a recovery summary, but it grants
//!    no trust. An active/pending disagreement can last until recovery; a peer
//!    completion message does not prove both devices have active trust.
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
/// observed to have written its own non-authoritative pending record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTrustRecord {
    pub hostname: String,
    /// base64 STANDARD of the peer's pinned Noise static public key.
    pub public_key: String,
    pub interface: String,
    pub address: String,
    /// Unix seconds at which this side recorded the confirmation.
    pub recorded_at: u64,
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
    writable: bool,
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
            writable: true,
        }
    }

    /// A missing store starts empty and writable. Unreadable, corrupt or future
    /// formats expose no records and remain read-only so a later mutation
    /// cannot overwrite recovery data that this build cannot interpret.
    pub fn load_from_path(path: &Path) -> Self {
        let mut writable = true;
        let records = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<StoreFile>(&text) {
                Ok(file) if file.format_version == STORE_FORMAT_VERSION => file.records,
                Ok(file) => {
                    writable = false;
                    log::warn!(
                        "Ignoring pending pairing store {} with unsupported format version {}",
                        path.display(),
                        file.format_version
                    );
                    Vec::new()
                }
                Err(error) => {
                    writable = false;
                    log::warn!(
                        "Ignoring unreadable pending pairing store {}: {error}",
                        path.display()
                    );
                    Vec::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                writable = false;
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
            writable,
        }
    }

    pub fn is_writable(&self) -> bool {
        self.writable
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
        let mut updated = self.clone();
        updated
            .records
            .retain(|known| known.hostname != record.hostname);
        updated.records.push(record);
        if let Err(error) = updated.save() {
            self.writable = false;
            return Err(error);
        }
        *self = updated;
        Ok(())
    }

    /// Drop the pending record. Called once the pairing became durable on both
    /// sides, or when the user forgets the device.
    pub fn remove(&mut self, hostname: &str) -> std::io::Result<()> {
        let mut updated = self.clone();
        updated.records.retain(|known| known.hostname != hostname);
        if let Err(error) = updated.save() {
            self.writable = false;
            return Err(error);
        }
        *self = updated;
        Ok(())
    }

    fn save(&self) -> std::io::Result<()> {
        if !self.writable {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,
                "Pending pairing store is unreadable or uses an unsupported format; preserving the original file"));
        }
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
