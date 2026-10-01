//! Private-filesystem primitives shared by identity, settings, and history
//! storage (T402, docs/SECURITY-AUDIT-2026-08.md).
//!
//! Everything TailSync manages on disk — identity keys, settings, the
//! history database, encrypted containers, and recovered clipboard files —
//! must be readable only by the owning user even though the payloads are
//! individually encrypted. This module centralizes the owner-only
//! permission policy so callers cannot drift:
//!
//! - Unix: directories `0o700`, files `0o600`, applied at creation time via
//!   `OpenOptionsExt::mode` (no create-wide-then-chmod window).
//! - Windows: a protected DACL (process user, owner, SYSTEM and
//!   administrators only) on app-managed objects; directory ACEs are
//!   inheritable so children created afterwards pick them up automatically.
//!
//! User-selected custom storage roots are deliberately *not* restricted:
//! only directories the application itself creates are locked.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(windows)]
static PRIVATE_FILE_RESTRICTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// A lease is installed before a writer creates its temporary. GC holds the same
// registry lock while unlinking, so an aged active writer cannot race cleanup.
static ATOMIC_WRITERS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

pub(crate) struct AtomicWriteLease(std::path::PathBuf);
impl AtomicWriteLease {
    pub(crate) fn acquire(path: &Path) -> Self {
        let key = atomic_temp_key(path);
        ATOMIC_WRITERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.clone());
        Self(key)
    }
}
impl Drop for AtomicWriteLease {
    fn drop(&mut self) {
        ATOMIC_WRITERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}
fn atomic_temp_key(path: &Path) -> std::path::PathBuf {
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Only recognized PID-bearing writer names can be collected. Legacy temps
/// without an owner PID and arbitrary .tmp files remain untouched.
pub(crate) fn collect_inactive_atomic_temp(path: &Path) -> io::Result<bool> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    let owner = if let Some(stem) = name
        .strip_prefix('.')
        .and_then(|name| name.strip_suffix(".tmp"))
    {
        stem.rsplit_once('.').map(|(_, owner)| owner)
    } else {
        name.rsplit_once(".tmp.").map(|(_, owner)| owner)
    };
    let Some((pid, nonce)) = owner.and_then(|owner| owner.split_once('-')) else {
        return Ok(false);
    };
    if nonce.len() != 16 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let Some(pid) = pid.parse::<u32>().ok().filter(|pid| *pid > 0) else {
        return Ok(false);
    };
    let writers = ATOMIC_WRITERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if writers.contains(&atomic_temp_key(path))
        || (pid != std::process::id() && process_may_be_running(pid))
    {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}
#[cfg(unix)]
fn process_may_be_running(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return true;
    };
    (unsafe { libc::kill(pid, 0) == 0 })
        || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
#[cfg(windows)]
fn process_may_be_running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return io::Error::last_os_error().raw_os_error() != Some(ERROR_INVALID_PARAMETER as i32);
    }
    let mut code = 0;
    let alive =
        unsafe { GetExitCodeProcess(process, &mut code) } == 0 || code == STILL_ACTIVE as u32;
    unsafe {
        CloseHandle(process);
    }
    alive
}
#[cfg(not(any(unix, windows)))]
fn process_may_be_running(_pid: u32) -> bool {
    true
}

/// Restrict an existing directory to the owning user.
pub fn restrict_private_dir(path: &Path) -> io::Result<()> {
    ensure_expected_type(path, true)?;
    restrict(path, true)
}

/// Restrict an existing regular file to the owning user.
pub fn restrict_private_file(path: &Path) -> io::Result<()> {
    // Windows rejects ACL updates that overlap another thread replacing the
    // same file. Keep link inspection, isolation, and restriction atomic.
    #[cfg(windows)]
    let _guard = PRIVATE_FILE_RESTRICTION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ensure_expected_type(path, false)?;
    if has_multiple_hard_links(path)? {
        break_hard_link(path)?;
    }
    restrict(path, false)
}

#[cfg(unix)]
fn has_multiple_hard_links(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::symlink_metadata(path)?.nlink() > 1)
}

#[cfg(windows)]
fn has_multiple_hard_links(path: &Path) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let file = File::open(path)?;
    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(information.nNumberOfLinks > 1)
    }
}

#[cfg(not(any(unix, windows)))]
fn has_multiple_hard_links(_path: &Path) -> io::Result<bool> {
    Ok(false)
}

fn break_hard_link(path: &Path) -> io::Result<()> {
    let (temporary, mut output, _lease) = allocate_private_temporary(path)?;
    let result = (|| -> io::Result<()> {
        let mut input = File::open(path)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        drop(input);
        replace_file_atomic(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn ensure_expected_type(path: &Path, directory: bool) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let expected = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if metadata.file_type().is_symlink() || !expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("private path has an unexpected type: {}", path.display()),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if directory { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(windows)]
fn restrict(path: &Path, directory: bool) -> io::Result<()> {
    set_private_windows_acl(path, directory)
}

#[cfg(not(any(unix, windows)))]
fn restrict(_path: &Path, _directory: bool) -> io::Result<()> {
    Ok(())
}

/// `create_dir_all` that locks every directory it creates. Directories that
/// already exist are left untouched, so a user-provided parent keeps its own
/// permissions.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        if current.as_os_str().is_empty() {
            return Ok(());
        }
        match std::fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "private directory cannot be a symlink: {}",
                        current.display()
                    ),
                ));
            }
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "private directory path is not a directory: {}",
                        current.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        missing.push(current.to_path_buf());
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    for directory in missing.iter().rev() {
        match create_private_directory(directory) {
            Ok(()) => {
                if let Err(error) = restrict_private_dir(directory) {
                    let _ = std::fs::remove_dir(directory);
                    return Err(error);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                ensure_expected_type(directory, true)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Open a new file for writing with owner-only permissions. Fails if the
/// file already exists (`create_new` semantics, matching the container
/// writers).
pub fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    let file = create_new_private_file(path)?;
    if let Err(error) = restrict_private_file(path) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

fn create_new_private_file(path: &Path) -> io::Result<File> {
    ensure_private_parent(path)?;
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }?;
    #[cfg(not(unix))]
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    Ok(file)
}

fn ensure_private_parent(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("private file has no parent directory: {}", path.display()),
        )
    })?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    ensure_expected_type(parent, true)
}

/// Open a managed file for read/write, creating it privately when absent and
/// rejecting symlinks or non-regular pre-existing paths when resuming.
pub fn open_private_file(path: &Path, truncate_existing: bool) -> io::Result<File> {
    ensure_private_parent(path)?;
    #[cfg(unix)]
    let create_result = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    };
    #[cfg(not(unix))]
    let create_result = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path);

    match create_result {
        Ok(file) => {
            if let Err(error) = restrict_private_file(path) {
                drop(file);
                let _ = std::fs::remove_file(path);
                return Err(error);
            }
            Ok(file)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            restrict_private_file(path)?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .truncate(truncate_existing)
                .open(path)
        }
        Err(error) => Err(error),
    }
}

/// Copy a regular file into a new owner-only file without sharing its inode.
/// Symlink sources are rejected so managed storage never imports data from
/// outside the path the caller validated.
pub fn copy_private_file(source: &Path, target: &Path) -> io::Result<u64> {
    ensure_expected_type(source, false)?;
    let mut reader = File::open(source)?;
    let mut writer = create_private_file(target)?;
    let copied = io::copy(&mut reader, &mut writer).and_then(|copied| {
        writer.sync_all()?;
        Ok(copied)
    });
    match copied {
        Ok(copied) => Ok(copied),
        Err(error) => {
            drop(writer);
            let _ = std::fs::remove_file(target);
            Err(error)
        }
    }
}

/// Atomically write `bytes` to `path` with owner-only permissions. The
/// temporary file uses a random suffix (never a fixed process-id name, which
/// would collide between concurrent writers in one process) and is fully
/// synced before the rename, and the parent directory is synced afterwards
/// on Unix so the rename itself is durable.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let (temporary, mut file, _lease) = allocate_private_temporary(path)?;
    let result = (|| -> io::Result<()> {
        let write = file.write_all(bytes).and_then(|_| file.sync_all());
        if let Err(error) = write {
            drop(file);
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        drop(file);
        replace_file_atomic(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn allocate_private_temporary(
    path: &Path,
) -> io::Result<(std::path::PathBuf, File, AtomicWriteLease)> {
    for _ in 0..8 {
        let temporary = path.with_extension(format!(
            "tmp.{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let lease = AtomicWriteLease::acquire(&temporary);
        match create_new_private_file(&temporary) {
            Ok(file) => {
                // create_new proves this path cannot already be a hard link.
                // Restrict it directly so hard-link isolation can allocate a
                // temporary while holding the Windows restriction lock.
                if let Err(error) = restrict(&temporary, false) {
                    drop(file);
                    let _ = std::fs::remove_file(&temporary);
                    return Err(error);
                }
                return Ok((temporary, file, lease));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a private temporary file",
    ))
}

#[cfg(target_os = "windows")]
fn replace_file_atomic(temporary: &Path, target: &Path) -> io::Result<()> {
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
    if unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomic(temporary: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(temporary, target)?;
    if let Some(parent) = target.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Startup repair: walk the managed tree and re-apply the
/// owner-only policy to anything that predates it (directories and files
/// created by older versions, or files created by SQLite under the umask).
/// Symlinks are never followed, and permission failures stop the open so the
/// application cannot silently continue with exposed private data.
pub fn enforce_private_tree(root: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "private storage root is not a regular directory: {}",
                    root.display()
                ),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    restrict_private_dir(root)?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "private storage contains a symbolic link: {}",
                        path.display()
                    ),
                ));
            }
            if file_type.is_dir() {
                restrict_private_dir(&path)?;
                stack.push(path);
            } else if file_type.is_file() {
                restrict_private_file(&path)?;
            }
        }
    }
    Ok(())
}

/// Restrict the SQLite database files, which rusqlite
/// creates under the process umask rather than through this module.
pub fn enforce_private_database(db_path: &Path) -> io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let path = if suffix.is_empty() {
            db_path.to_path_buf()
        } else {
            path_with_suffix(db_path, suffix)
        };
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                restrict_private_file(&path)?;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "SQLite private path has an unexpected type: {}",
                        path.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn path_with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// NTFS ACL inheritance propagation is asynchronous: while one thread
/// restricts a parent directory (or replaces a sibling), a concurrent
/// `SetFileSecurityW` on a file inside it can fail with a transient
/// ERROR_ACCESS_DENIED even though the caller holds WRITE_DAC. The
/// in-process restriction mutex removes most of the overlap, but parent
/// restriction and file replacement still churn the security descriptors,
/// so absorb the transient failures with a short bounded retry instead of
/// failing the whole create/restrict flow.
#[cfg(windows)]
fn set_private_windows_acl(path: &Path, directory: bool) -> io::Result<()> {
    const MAX_ATTEMPTS: u32 = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        match set_private_windows_acl_once(path, directory) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.raw_os_error() == Some(5) /* ERROR_ACCESS_DENIED */
                    && attempt < MAX_ATTEMPTS =>
            {
                std::thread::sleep(std::time::Duration::from_millis(u64::from(attempt) * 4));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("the retry loop always returns or errors on the last attempt")
}

#[cfg(windows)]
fn set_private_windows_acl_once(path: &Path, directory: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let user_sid = current_windows_user_sid()?;

    // OW is the Windows Owner Rights SID. An explicit process-user ACE keeps
    // ACL updates idempotent when Windows assigns a group as the object owner.
    let sddl = if directory {
        format!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{user_sid})(A;OICI;FA;;;OW)")
    } else {
        format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{user_sid})(A;;FA;;;OW)")
    };
    let sddl = sddl
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let result = unsafe {
        SetFileSecurityW(
            path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        )
    };
    unsafe {
        LocalFree(descriptor);
    }
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn current_windows_user_sid() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::Authorization::ConvertSidToStringSidW,
        Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let user_sid = (|| -> io::Result<String> {
        let mut required = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut required);
        }
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let token_user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut string_sid = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut string_sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = unsafe {
            let mut length = 0;
            while *string_sid.add(length) != 0 {
                length += 1;
            }
            length
        };
        let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(string_sid, length) })
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
        unsafe {
            LocalFree(string_sid.cast());
        }
        sid
    })();
    unsafe {
        CloseHandle(token);
    }
    user_sid
}

#[cfg(all(test, windows))]
pub(crate) fn assert_owner_only_windows_acl_for_test(path: &Path, directory: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            SDDL_REVISION_1,
        },
        Security::{
            GetAce, GetFileSecurityW, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            ACCESS_ALLOWED_ACE, ACE_HEADER, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
            OBJECT_INHERIT_ACE, SE_DACL_PROTECTED,
        },
    };
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut required = 0;
    unsafe {
        GetFileSecurityW(
            name.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
    }
    assert!(required > 0);
    let mut descriptor = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
    assert_ne!(
        unsafe {
            GetFileSecurityW(
                name.as_ptr(),
                DACL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        },
        0
    );
    let mut output = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor.as_ptr().cast_mut().cast(),
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut output,
                std::ptr::null_mut(),
            )
        },
        0
    );
    let mut length = 0;
    unsafe {
        while *output.add(length) != 0 {
            length += 1;
        }
    }
    let sddl = String::from_utf16(unsafe { std::slice::from_raw_parts(output, length) }).unwrap();
    unsafe {
        LocalFree(output.cast());
    }
    let descriptor_ptr = descriptor.as_mut_ptr().cast();
    let mut control = 0;
    let mut revision = 0;
    assert_ne!(
        unsafe { GetSecurityDescriptorControl(descriptor_ptr, &mut control, &mut revision) },
        0
    );
    assert_ne!(control & SE_DACL_PROTECTED, 0, "unprotected DACL: {sddl}");
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            GetSecurityDescriptorDacl(descriptor_ptr, &mut present, &mut dacl, &mut defaulted)
        },
        0
    );
    assert!(present != 0 && !dacl.is_null(), "missing DACL: {sddl}");
    let expected = std::collections::HashSet::from([
        current_windows_user_sid().unwrap(),
        "S-1-5-18".to_string(),     // SYSTEM
        "S-1-5-32-544".to_string(), // Administrators
        "S-1-3-4".to_string(),      // Owner Rights
    ]);
    let mut trustees = std::collections::HashSet::new();
    for index in 0..u32::from(unsafe { (*dacl).AceCount }) {
        let mut ace = std::ptr::null_mut();
        assert_ne!(unsafe { GetAce(dacl, index, &mut ace) }, 0);
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        // ACCESS_ALLOWED_ACE_TYPE is zero. Check the tag before reading SidStart.
        assert_eq!(header.AceType, 0, "unexpected ACE type: {sddl}");
        let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
        let sid = unsafe { std::ptr::addr_of!((*allowed).SidStart).cast_mut().cast() };
        let mut output = std::ptr::null_mut();
        assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut output) }, 0);
        let mut length = 0;
        unsafe {
            while *output.add(length) != 0 {
                length += 1;
            }
        }
        let trustee =
            String::from_utf16(unsafe { std::slice::from_raw_parts(output, length) }).unwrap();
        unsafe {
            LocalFree(output.cast());
        }
        // The descriptor's SDDL can abbreviate the process SID (for example
        // LA for the local Administrator). Compare actual binary ACE SIDs.
        assert!(
            expected.contains(&trustee),
            "unexpected access grant: {sddl}"
        );
        if directory {
            let inheritance = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
            assert_eq!(header.AceFlags & inheritance, inheritance);
        }
        trustees.insert(trustee);
    }
    assert_eq!(trustees, expected, "missing owner policy: {sddl}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn temporary_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tailsync-private-fs-{name}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn create_private_dir_all_locks_created_directories() {
        let root = temporary_root("dirs");
        let managed = root.join("TailSync Data").join("file-history");
        create_private_dir_all(&managed).unwrap();
        assert!(managed.is_dir());
        #[cfg(unix)]
        {
            assert_eq!(mode_of(&managed), 0o700);
            assert_eq!(mode_of(managed.parent().unwrap()), 0o700);
        }
        // Idempotent on an existing tree.
        create_private_dir_all(&managed).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_private_dir_all_leaves_existing_directories_alone() {
        let root = temporary_root("existing");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        create_private_dir_all(&root).unwrap();
        #[cfg(unix)]
        assert_eq!(mode_of(&root), 0o755);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_private_file_is_owner_only_and_fails_on_existing() {
        let root = temporary_root("files");
        let path = root.join("container.bin");
        {
            let mut file = create_private_file(&path).unwrap();
            file.write_all(b"payload").unwrap();
        }
        #[cfg(unix)]
        assert_eq!(mode_of(&path), 0o600);
        assert!(create_private_file(&path).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn private_windows_acl_can_be_reapplied() {
        let root = temporary_root("windows-acl-idempotence");
        let directory = root.join("managed");
        create_private_dir_all(&directory).unwrap();
        restrict_private_dir(&directory).unwrap();
        restrict_private_dir(&directory).unwrap();

        let path = directory.join("secret.bin");
        drop(create_private_file(&path).unwrap());
        restrict_private_file(&path).unwrap();
        restrict_private_file(&path).unwrap();
        std::fs::write(&path, b"still accessible").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"still accessible");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn owner_acl_assertion_rejects_everyone_access() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            Security::{
                SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            },
        };
        let root = temporary_root("windows-acl-unexpected-trustee");
        let path = root.join("empty.bin");
        drop(create_private_file(&path).unwrap());
        assert_owner_only_windows_acl_for_test(&path, false);

        let sddl: Vec<_> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
        let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let result = unsafe {
            SetFileSecurityW(
                name.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        assert_ne!(result, 0);
        let rejected = std::panic::catch_unwind(|| {
            assert_owner_only_windows_acl_for_test(&path, false);
        });
        restrict_private_file(&path).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        let panic = rejected.expect_err("Everyone access must fail the owner-only assertion");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(message.contains("unexpected access grant"), "{message}");
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_private_file_restriction_isolates_a_hard_link_once() {
        let root = temporary_root("windows-concurrent-hard-link");
        let managed = root.join("managed");
        create_private_dir_all(&managed).unwrap();
        let source = root.join("source.bin");
        std::fs::write(&source, b"original").unwrap();
        let path = managed.join("secret.bin");
        std::fs::hard_link(&source, &path).unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let handles = (0..16)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    restrict_private_file(&path)
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        std::fs::write(&path, b"managed copy").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn write_private_file_is_atomic_and_owner_only() {
        let root = temporary_root("atomic");
        let path = root.join("config.json");
        write_private_file(&path, b"{\"a\":1}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
        #[cfg(unix)]
        assert_eq!(mode_of(&path), 0o600);
        // No temporary leftovers.
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1);
        // Overwrite in place keeps the same guarantees.
        write_private_file(&path, b"{\"a\":2}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":2}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn enforce_private_tree_repairs_pre_existing_paths() {
        use std::os::unix::fs::PermissionsExt;
        let root = temporary_root("repair");
        let child = root.join("container");
        std::fs::create_dir_all(&child).unwrap();
        let wide = root.join("legacy.bin");
        std::fs::write(&wide, b"x").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o644)).unwrap();

        enforce_private_tree(&root).unwrap();

        assert_eq!(mode_of(&root), 0o700);
        assert_eq!(mode_of(&child), 0o700);
        assert_eq!(mode_of(&wide), 0o600);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enforce_private_database_handles_missing_files() {
        let root = temporary_root("db");
        enforce_private_database(&root.join("history-v2.db")).unwrap();
        let db = root.join("history-v2.db");
        std::fs::write(&db, b"sqlite").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        enforce_private_database(&db).unwrap();
        #[cfg(unix)]
        assert_eq!(mode_of(&db), 0o600);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn create_private_dir_all_rejects_a_symlink_root() {
        let root = temporary_root("create-symlink-root");
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let managed = root.join("managed");
        std::os::unix::fs::symlink(&outside, &managed).unwrap();

        assert!(create_private_dir_all(&managed).is_err());

        std::fs::remove_file(managed).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn enforce_private_tree_does_not_follow_a_symlink_root() {
        use std::os::unix::fs::PermissionsExt;

        let root = temporary_root("enforce-symlink-root");
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let outside_file = outside.join("public.bin");
        std::fs::write(&outside_file, b"outside").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&outside_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let managed = root.join("managed");
        std::os::unix::fs::symlink(&outside, &managed).unwrap();

        assert!(enforce_private_tree(&managed).is_err());

        assert_eq!(mode_of(&outside), 0o755);
        assert_eq!(mode_of(&outside_file), 0o644);
        std::fs::remove_file(managed).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_file_creation_rejects_a_symlink_parent() {
        let root = temporary_root("file-symlink-parent");
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let managed = root.join("managed");
        std::os::unix::fs::symlink(&outside, &managed).unwrap();

        assert!(create_private_file(&managed.join("secret.bin")).is_err());
        assert!(!outside.join("secret.bin").exists());

        std::fs::remove_file(managed).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn enforce_private_tree_rejects_child_symlinks() {
        let root = temporary_root("enforce-child-symlink");
        let outside = root.join("outside.bin");
        std::fs::write(&outside, b"outside").unwrap();
        let link = root.join("linked.bin");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        assert!(enforce_private_tree(&root).is_err());

        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enforce_private_tree_breaks_legacy_hard_links_without_touching_the_source() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let root = temporary_root("enforce-legacy-hard-link");
        let managed = root.join("managed");
        std::fs::create_dir(&managed).unwrap();
        let source = root.join("user-file.txt");
        std::fs::write(&source, b"original").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o644)).unwrap();
        let legacy = managed.join("legacy-link.txt");
        std::fs::hard_link(&source, &legacy).unwrap();

        enforce_private_tree(&managed).unwrap();

        #[cfg(unix)]
        {
            assert_eq!(mode_of(&source), 0o644);
            assert_eq!(mode_of(&legacy), 0o600);
        }
        std::fs::write(&legacy, b"managed copy").unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"original");

        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod atomic_temp_lifecycle_tests {
    use super::*;
    #[test]
    fn atomic_writer_exit_probe() {
        let Some(directory) = std::env::var_os("TAILSYNC_ATOMIC_EXIT_PROBE") else {
            return;
        };
        let path = std::path::PathBuf::from(directory).join(format!(
            ".payload.{}-0123456789abcdef.tmp",
            std::process::id()
        ));
        let _lease = AtomicWriteLease::acquire(&path);
        let mut file = create_private_file(&path).unwrap();
        file.write_all(b"crash residue").unwrap();
        file.sync_all().unwrap();
        // Give the parent a live foreign writer to inspect, then exit without Drop.
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(74);
    }
    #[test]
    fn dead_writer_temp_is_collectible_after_a_process_restart() {
        let root =
            std::env::temp_dir().join(format!("tailsync-dead-writer-{}", rand::random::<u64>()));
        create_private_dir_all(&root).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "private_fs::atomic_temp_lifecycle_tests::atomic_writer_exit_probe",
            ])
            .env("TAILSYNC_ATOMIC_EXIT_PROBE", &root)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let path = root.join(format!(".payload.{}-0123456789abcdef.tmp", child.id()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !path.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            !collect_inactive_atomic_temp(&path).unwrap(),
            "foreign live writer was removed"
        );
        assert_eq!(child.wait().unwrap().code(), Some(74));
        assert!(collect_inactive_atomic_temp(&path).unwrap());
        assert!(!path.exists());
        // A running foreign process must be protected even without our registry.
        let live = root.join(format!(
            ".payload.{}-0123456789abcdef.tmp",
            std::process::id()
        ));
        std::fs::write(&live, b"active").unwrap();
        let lease = AtomicWriteLease::acquire(&live);
        assert!(!collect_inactive_atomic_temp(&live).unwrap());
        drop(lease);
        std::fs::remove_dir_all(root).unwrap();
    }
}
