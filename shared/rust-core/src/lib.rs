#[cfg(all(feature = "acceptance-injection", not(debug_assertions)))]
compile_error!("acceptance-injection must never be enabled in a release build");

pub mod cancellation;
pub mod crypto;
pub mod db;
pub mod diagnostics;
pub mod history_classifier;
pub mod identity;
pub mod import;
pub mod iroh_transport;
pub mod observability;
pub mod pairing;
pub mod peer;
pub mod private_fs;
pub mod protocol;
pub mod secure;
pub mod sync;
pub mod sync_warning;
pub mod themes_v2;
pub mod updates;
