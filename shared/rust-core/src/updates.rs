//! Shared signed-update metadata rules.
//!
//! Archive extraction remains a platform adapter because Windows and macOS
//! packages use different containers. The metadata schema, product identity,
//! and exact-version rule are transport-independent and therefore live here.

use serde::Deserialize;

pub const PACKAGE_METADATA_SCHEMA: u8 = 1;
pub const PACKAGE_PRODUCT: &str = "TailSync";

#[derive(Debug, Deserialize)]
struct PackageMetadata {
    schema: u8,
    product: String,
    version: String,
}

/// Validate the signed metadata embedded in a downloaded update package.
///
/// The updater plugin verifies the package signature before this function is
/// called. This second gate prevents accepting a validly signed package for a
/// different product, schema, or release version.
pub fn validate_package_metadata(metadata: &[u8], expected_version: &str) -> Result<(), String> {
    let metadata: PackageMetadata = serde_json::from_slice(metadata)
        .map_err(|error| format!("Invalid signed update metadata: {error}"))?;
    if metadata.schema != PACKAGE_METADATA_SCHEMA || metadata.product != PACKAGE_PRODUCT {
        return Err(format!(
            "The signed update package is not a {PACKAGE_PRODUCT} v{PACKAGE_METADATA_SCHEMA} package"
        ));
    }
    if metadata.version != expected_version {
        return Err(format!(
            "Refusing update downgrade or substitution: manifest version {expected_version}, package version {}",
            metadata.version
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_requires_the_current_product_and_schema() {
        let valid = br#"{"schema":1,"product":"TailSync","version":"2.2.2"}"#;
        assert!(validate_package_metadata(valid, "2.2.2").is_ok());

        let wrong_schema = br#"{"schema":2,"product":"TailSync","version":"2.2.2"}"#;
        assert!(validate_package_metadata(wrong_schema, "2.2.2")
            .unwrap_err()
            .contains("not a TailSync v1 package"));

        let wrong_product = br#"{"schema":1,"product":"Other","version":"2.2.2"}"#;
        assert!(validate_package_metadata(wrong_product, "2.2.2")
            .unwrap_err()
            .contains("not a TailSync v1 package"));
    }

    #[test]
    fn metadata_version_must_match_the_downloaded_release() {
        let metadata = br#"{"schema":1,"product":"TailSync","version":"2.1.0"}"#;
        let error = validate_package_metadata(metadata, "2.2.0").unwrap_err();
        assert!(error.contains("Refusing update downgrade or substitution"));
    }
}
