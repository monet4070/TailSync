use base64::{engine::general_purpose::STANDARD, Engine as _};
use minisign_verify::{PublicKey, Signature};
use std::{env, fs, path::PathBuf, process};

fn usage() -> ! {
    eprintln!("Usage: tailsync-updater-verify --public-key FILE --artifact FILE --signature FILE");
    process::exit(2);
}

fn option(args: &[String], name: &str) -> PathBuf {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        usage();
    };
    args.get(index + 1)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| usage())
}

/// Tauri stores minisign text as one outer Base64 value in both updater.pub
/// and .sig files. Decode exactly once before handing the text to the
/// verifier. Accepting raw minisign text here would hide a packaging format
/// regression, so malformed/double-encoded files fail closed.
fn decode_tauri_text(path: &PathBuf, label: &str) -> Result<String, String> {
    let encoded = fs::read_to_string(path)
        .map_err(|error| format!("could not read {label} {}: {error}", path.display()))?;
    let encoded = encoded.trim();
    if encoded.is_empty() {
        return Err(format!("{label} {} is empty", path.display()));
    }
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|error| format!("{label} {} is not outer Base64: {error}", path.display()))?;
    String::from_utf8(decoded).map_err(|error| {
        format!(
            "{label} {} is not UTF-8 minisign text: {error}",
            path.display()
        )
    })
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    let public_key_path = option(&args, "--public-key");
    let artifact_path = option(&args, "--artifact");
    let signature_path = option(&args, "--signature");

    let public_key_text = decode_tauri_text(&public_key_path, "public key")?;
    let signature_text = decode_tauri_text(&signature_path, "signature")?;
    let public_key = PublicKey::decode(&public_key_text)
        .map_err(|error| format!("invalid updater public key: {error}"))?;
    let signature = Signature::decode(&signature_text)
        .map_err(|error| format!("invalid updater signature: {error}"))?;
    let artifact = fs::read(&artifact_path).map_err(|error| {
        format!(
            "could not read artifact {}: {error}",
            artifact_path.display()
        )
    })?;
    public_key
        .verify(&artifact, &signature, false)
        .map_err(|error| {
            format!(
                "signature verification failed for {}: {error}",
                artifact_path.display()
            )
        })?;
    println!("verified {}", artifact_path.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}
