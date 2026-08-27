use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::hkdf;
use ring::rand::{SecureRandom, SystemRandom};

use crate::crypto;

const MAGIC: &[u8; 8] = b"TSFENC1\0";
const CHUNK_SIZE: usize = 1024 * 1024;
const TAG_SIZE: u64 = 16;
const SALT_SIZE: usize = 16;
const HASH_SIZE: usize = 32;
const HEADER_SIZE: usize = MAGIC.len() + 4 + 8 + SALT_SIZE + HASH_SIZE;
const HEADER_NONCE_INDEX: u32 = u32::MAX;
const KEY_INFO: &[u8] = b"tailsync-file-history-v1";

struct FileKeyLength;

impl hkdf::KeyType for FileKeyLength {
    fn len(&self) -> usize {
        32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileHeader {
    chunk_size: u32,
    plaintext_size: u64,
    salt: [u8; SALT_SIZE],
    plaintext_hash: [u8; HASH_SIZE],
}

impl FileHeader {
    fn new(plaintext_size: u64, data_hash: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let plaintext_hash = decode_hash(data_hash)?;
        let mut salt = [0_u8; SALT_SIZE];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| "could not generate file-history salt")?;
        Ok(Self {
            chunk_size: CHUNK_SIZE as u32,
            plaintext_size,
            salt,
            plaintext_hash,
        })
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_SIZE);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.chunk_size.to_le_bytes());
        bytes.extend_from_slice(&self.plaintext_size.to_le_bytes());
        bytes.extend_from_slice(&self.salt);
        bytes.extend_from_slice(&self.plaintext_hash);
        bytes
    }

    fn decode(bytes: &[u8; HEADER_SIZE]) -> Result<Self, Box<dyn std::error::Error>> {
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err("file-history container has an invalid magic value".into());
        }
        let mut offset = MAGIC.len();
        let chunk_size = u32::from_le_bytes(bytes[offset..offset + 4].try_into()?);
        offset += 4;
        let plaintext_size = u64::from_le_bytes(bytes[offset..offset + 8].try_into()?);
        offset += 8;
        let salt = bytes[offset..offset + SALT_SIZE].try_into()?;
        offset += SALT_SIZE;
        let plaintext_hash = bytes[offset..offset + HASH_SIZE].try_into()?;

        if chunk_size as usize != CHUNK_SIZE {
            return Err(format!("unsupported file-history chunk size: {chunk_size}").into());
        }
        super::file_storage::validate_history_file_size(plaintext_size)?;
        expected_container_size(plaintext_size)?;
        Ok(Self {
            chunk_size,
            plaintext_size,
            salt,
            plaintext_hash,
        })
    }
}

fn decode_hash(data_hash: &str) -> Result<[u8; HASH_SIZE], Box<dyn std::error::Error>> {
    let decoded = hex::decode(data_hash)?;
    decoded
        .try_into()
        .map_err(|_| "file-history hash must encode exactly 32 bytes".into())
}

fn derive_key(salt_bytes: &[u8; SALT_SIZE]) -> Result<LessSafeKey, Box<dyn std::error::Error>> {
    let dek = crypto::get_dek()?;
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt_bytes);
    let prk = salt.extract(&dek);
    let info = [KEY_INFO];
    let okm = prk
        .expand(&info, FileKeyLength)
        .map_err(|_| "could not derive file-history key")?;
    let mut key_bytes = [0_u8; 32];
    okm.fill(&mut key_bytes)
        .map_err(|_| "could not materialize file-history key")?;
    let unbound =
        UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| "invalid file-history key")?;
    Ok(LessSafeKey::new(unbound))
}

fn nonce(index: u32) -> Nonce {
    let mut bytes = [0_u8; 12];
    bytes[8..].copy_from_slice(&index.to_be_bytes());
    Nonce::assume_unique_for_key(bytes)
}

fn header_aad(header_bytes: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header_bytes.len() + 6);
    aad.extend_from_slice(header_bytes);
    aad.extend_from_slice(b"header");
    aad
}

fn chunk_aad(header_bytes: &[u8], index: u32, plaintext_length: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header_bytes.len() + 8);
    aad.extend_from_slice(header_bytes);
    aad.extend_from_slice(&index.to_be_bytes());
    aad.extend_from_slice(&plaintext_length.to_be_bytes());
    aad
}

fn expected_container_size(plaintext_size: u64) -> Result<u64, Box<dyn std::error::Error>> {
    let chunks = plaintext_size.div_ceil(CHUNK_SIZE as u64);
    let tag_bytes = chunks
        .checked_add(1)
        .and_then(|count| count.checked_mul(TAG_SIZE))
        .ok_or("file-history container size overflow")?;
    (HEADER_SIZE as u64)
        .checked_add(plaintext_size)
        .and_then(|size| size.checked_add(tag_bytes))
        .ok_or_else(|| "file-history container size overflow".into())
}

fn write_container(
    reader: &mut impl Read,
    output: &mut File,
    plaintext_size: u64,
    data_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    super::file_storage::validate_history_file_size(plaintext_size)?;
    let header = FileHeader::new(plaintext_size, data_hash)?;
    let header_bytes = header.encode();
    let key = derive_key(&header.salt)?;
    output.write_all(&header_bytes)?;

    let mut header_tag = Vec::new();
    key.seal_in_place_append_tag(
        nonce(HEADER_NONCE_INDEX),
        Aad::from(header_aad(&header_bytes)),
        &mut header_tag,
    )
    .map_err(|_| "could not authenticate file-history header")?;
    output.write_all(&header_tag)?;

    let mut remaining = plaintext_size;
    let mut chunk_index = 0_u32;
    let mut buffer = vec![0_u8; CHUNK_SIZE];
    let mut hasher = blake3::Hasher::new();
    while remaining > 0 {
        let length = usize::try_from(remaining.min(CHUNK_SIZE as u64))?;
        reader.read_exact(&mut buffer[..length])?;
        hasher.update(&buffer[..length]);
        let mut encrypted = buffer[..length].to_vec();
        key.seal_in_place_append_tag(
            nonce(chunk_index),
            Aad::from(chunk_aad(&header_bytes, chunk_index, length as u32)),
            &mut encrypted,
        )
        .map_err(|_| "could not encrypt file-history chunk")?;
        output.write_all(&encrypted)?;
        remaining -= length as u64;
        chunk_index = chunk_index
            .checked_add(1)
            .ok_or("file-history chunk index overflow")?;
    }

    let mut trailing = [0_u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err("file-history source grew while it was encrypted".into());
    }
    if hasher.finalize().as_bytes() != &header.plaintext_hash {
        return Err("file-history source does not match its BLAKE3 hash".into());
    }
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

fn read_header(input: &mut File) -> Result<(FileHeader, Vec<u8>), Box<dyn std::error::Error>> {
    let mut bytes = [0_u8; HEADER_SIZE];
    input.read_exact(&mut bytes)?;
    let header = FileHeader::decode(&bytes)?;
    Ok((header, bytes.to_vec()))
}

fn probe_header(path: &Path) -> Result<Option<FileHeader>, Box<dyn std::error::Error>> {
    let mut input = File::open(path)?;
    let mut magic = [0_u8; MAGIC.len()];
    let count = input.read(&mut magic)?;
    if count != MAGIC.len() || &magic != MAGIC {
        return Ok(None);
    }
    let mut remainder = [0_u8; HEADER_SIZE - MAGIC.len()];
    input.read_exact(&mut remainder)?;
    let mut bytes = [0_u8; HEADER_SIZE];
    bytes[..MAGIC.len()].copy_from_slice(&magic);
    bytes[MAGIC.len()..].copy_from_slice(&remainder);
    Ok(Some(FileHeader::decode(&bytes)?))
}

pub(super) fn encrypted_file_matches(
    path: &Path,
    plaintext_size: u64,
    data_hash: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut input = File::open(path)?;
    let mut magic = [0_u8; MAGIC.len()];
    let count = input.read(&mut magic)?;
    if count != MAGIC.len() || &magic != MAGIC {
        return Ok(false);
    }
    input.rewind()?;
    let (header, header_bytes) = read_header(&mut input)?;
    let key = derive_key(&header.salt)?;
    let mut header_tag = vec![0_u8; TAG_SIZE as usize];
    input.read_exact(&mut header_tag)?;
    key.open_in_place(
        nonce(HEADER_NONCE_INDEX),
        Aad::from(header_aad(&header_bytes)),
        &mut header_tag,
    )
    .map_err(|_| "file-history header authentication failed")?;
    let expected_hash = decode_hash(data_hash)?;
    let expected_size = expected_container_size(plaintext_size)?;
    Ok(header.plaintext_size == plaintext_size
        && header.plaintext_hash == expected_hash
        && path.metadata()?.len() == expected_size)
}

pub(super) fn is_encrypted_file(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(probe_header(path)?.is_some())
}

fn temporary_path(target: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let parent = target
        .parent()
        .ok_or("file-history target has no parent directory")?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("file-history target name is not valid UTF-8")?;
    for _ in 0..8 {
        let candidate = parent.join(format!(
            ".{name}.{}-{:016x}.tmp",
            std::process::id(),
            rand::random::<u64>()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("could not allocate a file-history temporary path".into())
}

#[cfg(target_os = "windows")]
fn replace_file_atomic(temporary: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomic(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)?;
    if let Some(parent) = target.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn encrypt_reader_atomic(
    reader: &mut impl Read,
    plaintext_size: u64,
    data_hash: &str,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = target
        .parent()
        .ok_or("file-history target has no parent directory")?;
    crate::private_fs::create_private_dir_all(parent)?;
    let temporary = temporary_path(target)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut output = crate::private_fs::create_private_file(&temporary)?;
        write_container(reader, &mut output, plaintext_size, data_hash)?;
        drop(output);
        replace_file_atomic(&temporary, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub(super) fn encrypt_bytes_atomic(
    data: &[u8],
    data_hash: &str,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    encrypt_reader_atomic(&mut Cursor::new(data), data.len() as u64, data_hash, target)
}

pub(super) fn encrypt_file_atomic(
    source: &Path,
    plaintext_size: u64,
    data_hash: &str,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = File::open(source)?;
    encrypt_reader_atomic(&mut input, plaintext_size, data_hash, target)
}

pub(super) fn ensure_file_encrypted(
    path: &Path,
    plaintext_size: u64,
    data_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if probe_header(path)?.is_some() {
        if encrypted_file_matches(path, plaintext_size, data_hash)? {
            return Ok(());
        }
        return Err("encrypted file-history metadata does not match the database".into());
    }
    if path.metadata()?.len() != plaintext_size {
        return Err("legacy file-history size does not match the database".into());
    }
    let temporary = temporary_path(path)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut input = File::open(path)?;
        let mut output = crate::private_fs::create_private_file(&temporary)?;
        write_container(&mut input, &mut output, plaintext_size, data_hash)?;
        drop(output);
        drop(input);
        replace_file_atomic(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn decrypt_to_writer(
    source: &Path,
    output: &mut impl Write,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut input = File::open(source)?;
    let (header, header_bytes) = read_header(&mut input)?;
    if input.metadata()?.len() != expected_container_size(header.plaintext_size)? {
        return Err("file-history container length is invalid".into());
    }
    let key = derive_key(&header.salt)?;

    let mut header_tag = vec![0_u8; TAG_SIZE as usize];
    input.read_exact(&mut header_tag)?;
    key.open_in_place(
        nonce(HEADER_NONCE_INDEX),
        Aad::from(header_aad(&header_bytes)),
        &mut header_tag,
    )
    .map_err(|_| "file-history header authentication failed")?;

    let mut remaining = header.plaintext_size;
    let mut chunk_index = 0_u32;
    let mut hasher = blake3::Hasher::new();
    while remaining > 0 {
        let plaintext_length = usize::try_from(remaining.min(CHUNK_SIZE as u64))?;
        let mut encrypted = vec![0_u8; plaintext_length + TAG_SIZE as usize];
        input.read_exact(&mut encrypted)?;
        let plaintext = key
            .open_in_place(
                nonce(chunk_index),
                Aad::from(chunk_aad(
                    &header_bytes,
                    chunk_index,
                    plaintext_length as u32,
                )),
                &mut encrypted,
            )
            .map_err(|_| format!("file-history chunk {chunk_index} authentication failed"))?;
        hasher.update(plaintext);
        output.write_all(plaintext)?;
        remaining -= plaintext_length as u64;
        chunk_index = chunk_index
            .checked_add(1)
            .ok_or("file-history chunk index overflow")?;
    }
    if hasher.finalize().as_bytes() != &header.plaintext_hash {
        return Err("file-history plaintext hash mismatch".into());
    }
    output.flush()?;
    Ok(header.plaintext_size)
}

pub(super) fn decrypt_file_to_vec(source: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let capacity = probe_header(source)?
        .ok_or("file-history file is not encrypted")?
        .plaintext_size
        .try_into()?;
    let mut output = Vec::with_capacity(capacity);
    decrypt_to_writer(source, &mut output)?;
    Ok(output)
}

pub(super) fn decrypt_file_to_path(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // The decrypted payload is plaintext on disk (e.g. a restored clipboard
    // file): it must be owner-only from the moment it is created.
    let mut output = crate::private_fs::create_private_file(target)?;
    let result = decrypt_to_writer(source, &mut output).and_then(|_| {
        output.sync_all()?;
        Ok(())
    });
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tailsync-{name}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn encrypted_container_round_trips_multiple_chunks() {
        let root = temporary_root("file-encryption");
        let target = root.join("history.bin");
        let data = (0..(CHUNK_SIZE * 2 + 317))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let hash = blake3::hash(&data).to_hex().to_string();

        encrypt_bytes_atomic(&data, &hash, &target).unwrap();
        assert!(encrypted_file_matches(&target, data.len() as u64, &hash).unwrap());
        let stored = std::fs::read(&target).unwrap();
        assert!(!stored.windows(64).any(|window| window == &data[..64]));
        assert_eq!(decrypt_file_to_vec(&target).unwrap(), data);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupted_chunk_is_rejected_without_plaintext_output() {
        let root = temporary_root("file-corruption");
        let target = root.join("history.bin");
        let output = root.join("restored.bin");
        let data = vec![0x5a; CHUNK_SIZE + 10];
        let hash = blake3::hash(&data).to_hex().to_string();
        encrypt_bytes_atomic(&data, &hash, &target).unwrap();
        let mut stored = std::fs::read(&target).unwrap();
        let last = stored.last_mut().unwrap();
        *last ^= 0x80;
        std::fs::write(&target, stored).unwrap();

        assert!(decrypt_file_to_path(&target, &output).is_err());
        assert!(!output.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plaintext_migration_is_idempotent() {
        let root = temporary_root("file-migration");
        let target = root.join("legacy.bin");
        let data = b"legacy plaintext file";
        let hash = blake3::hash(data).to_hex().to_string();
        std::fs::write(&target, data).unwrap();

        ensure_file_encrypted(&target, data.len() as u64, &hash).unwrap();
        ensure_file_encrypted(&target, data.len() as u64, &hash).unwrap();
        assert_eq!(decrypt_file_to_vec(&target).unwrap(), data);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hash_mismatch_never_installs_a_container() {
        let root = temporary_root("file-hash-mismatch");
        let target = root.join("history.bin");
        let wrong_hash = blake3::hash(b"different").to_hex().to_string();
        assert!(encrypt_bytes_atomic(b"payload", &wrong_hash, &target).is_err());
        assert!(!target.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
