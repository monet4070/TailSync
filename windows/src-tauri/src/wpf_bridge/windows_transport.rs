use super::*;
use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

struct OwnedHandle(HANDLE);
// Owns the kernel handle exclusively. Windows permits using/closing it from
// another thread; this type never wraps a borrowed application handle.
unsafe impl Send for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        let mut required = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        let mut storage = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*(storage.as_ptr().cast::<TOKEN_USER>());
        let mut sid = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *sid.add(length) != 0 {
            length += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(sid, length));
        LocalFree(sid.cast());
        Ok(text)
    }
}

fn create_pipe(path: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sid = current_user_sid()?;
    // Protected DACL grants access only to the current user. No inherited
    // Everyone/Users ACE; remote named-pipe clients are separately rejected.
    let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{sid})\0").encode_utf16().collect();
    unsafe {
        let mut security_descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security_descriptor,
            null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security_descriptor,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(MAX_CONNECTIONS + 1)
            .in_buffer_size(64 * 1024)
            .out_buffer_size(64 * 1024)
            .create_with_security_attributes_raw(path, &attributes as *const _ as *mut c_void);
        LocalFree(security_descriptor);
        result
    }
}

pub(super) fn allow_frontend_activation() {
    if let Some(parent_pid) = CONFIGURATION.get().and_then(|config| config.parent_pid) {
        // Never grant ASFW_ANY: only the authenticated session's parent owns UI.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(parent_pid);
        }
    }
}

pub(super) fn validate_parent(parent_pid: u32) -> Result<(), String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("Could not verify WPF parent process".into());
        }
        let snapshot = OwnedHandle(snapshot);
        let mut process: PROCESSENTRY32W = zeroed();
        process.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let own_pid = GetCurrentProcessId();
        let mut found = Process32FirstW(snapshot.0, &mut process) != 0;
        while found {
            if process.th32ProcessID == own_pid {
                return if process.th32ParentProcessID == parent_pid {
                    Ok(())
                } else {
                    Err("Requested WPF parent is not the process parent".into())
                };
            }
            found = Process32NextW(snapshot.0, &mut process) != 0;
        }
    }
    Err("Could not verify WPF parent process".into())
}

pub(super) fn start(app: AppHandle, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    let Some(config) = CONFIGURATION.get() else {
        return Ok(());
    };
    // Setup runs on the native event-loop thread; attaching its overlapped
    // pipe handle needs an entered Tokio reactor even before our tasks start.
    let runtime = tauri::async_runtime::handle();
    let _runtime_guard = runtime.inner().enter();
    let path = format!(r"\\.\pipe\{}", config.pipe);
    let mut server = create_pipe(&path, true)?;
    let last_request = Arc::new(Mutex::new(Instant::now()));
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let monitor_app = app.clone();
    let monitor_contact = last_request.clone();
    let mut monitor_shutdown = shutdown.clone();
    let parent = if let Some(pid) = config.parent_pid {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Some(handle as usize)
    } else {
        None
    };
    tauri::async_runtime::spawn(async move {
        let parent = parent.map(|handle| OwnedHandle(handle as HANDLE));
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                _ = monitor_shutdown.changed() => break,
            }
            let parent_alive = parent.as_ref().is_none_or(|process| unsafe {
                let mut exit_code = 0;
                GetExitCodeProcess(process.0, &mut exit_code) != 0
                    && exit_code == STILL_ACTIVE as u32
            });
            let expired = monitor_contact
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .elapsed()
                > Duration::from_secs(45);
            if !parent_alive || expired {
                log::info!("WPF frontend session ended; stopping its backend");
                let _ = monitor_app.state::<crate::AppState>().shutdown.send(true);
                break;
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        loop {
            let permit = tokio::select! {
                permit = semaphore.clone().acquire_owned() => match permit { Ok(permit) => permit, Err(_) => break },
                _ = shutdown.changed() => break,
            };
            tokio::select! {
                connected = server.connect() => if connected.is_err() { break; },
                _ = shutdown.changed() => break,
            }
            // Make the next acceptor before transferring this instance, avoiding
            // a missing-pipe gap for concurrent UI commands and long polling.
            let next = match create_pipe(&path, false) {
                Ok(next) => next,
                Err(_) => {
                    let _ = app.state::<crate::AppState>().shutdown.send(true);
                    break;
                }
            };
            let connected = server;
            server = next;
            let app = app.clone();
            let contact = last_request.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let _ =
                    tokio::time::timeout(Duration::from_secs(135), serve(connected, app, contact))
                        .await;
            });
        }
    });
    log::info!("Native WPF backend transport ready");
    Ok(())
}

async fn read_request<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> io::Result<Request> {
    let length = reader.read_u32_le().await? as usize;
    if length == 0 || length > MAX_JSON_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid request frame",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid request"))
}

async fn serve(
    mut pipe: NamedPipeServer,
    app: AppHandle,
    contact: Arc<Mutex<Instant>>,
) -> io::Result<()> {
    let request: Request = tokio::time::timeout(Duration::from_secs(15), read_request(&mut pipe))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Request timed out"))??;
    let config = CONFIGURATION
        .get()
        .ok_or_else(|| io::Error::other("Session unavailable"))?;
    let outcome = if authenticated(&config.token, &request.token) {
        *contact
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
        match tokio::time::timeout(
            Duration::from_secs(120),
            dispatch(&app, &request.command, request.args),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => Err(stable_error(
                tailsync_runtime::contracts::StableErrorCode::TemporarilyBusy,
            )),
        }
    } else {
        Err(stable_error(
            tailsync_runtime::contracts::StableErrorCode::Unauthorized,
        ))
    };
    let (body, binary) = match outcome {
        Ok(reply) => (
            json!({"id":request.id,"ok":true,"result":reply.result,"binaryLength":reply.binary.as_ref().map(Vec::len).unwrap_or(0)}),
            reply.binary,
        ),
        Err(error) => (
            json!({"id":request.id,"ok":false,"error":error,"binaryLength":0}),
            None,
        ),
    };
    let bytes = serde_json::to_vec(&body)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Response too large",
        ));
    }
    pipe.write_u32_le(bytes.len() as u32).await?;
    pipe.write_all(&bytes).await?;
    if let Some(binary) = binary {
        pipe.write_all(&binary).await?;
    }
    pipe.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn current_user_pipe_accepts_client_and_rejects_name_collision() {
        let path = format!(r"\\.\pipe\TailSync.Wpf.{:032x}", rand::random::<u128>());
        let pipe = create_pipe(&path, true).unwrap();
        assert!(create_pipe(&path, true).is_err());
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&path)
            .unwrap();
        pipe.connect().await.unwrap();
        let mut server = pipe;
        client.write_u32_le(17).await.unwrap();
        assert_eq!(server.read_u32_le().await.unwrap(), 17);
    }
    #[tokio::test]
    async fn request_frame_rejects_oversize_and_truncation_before_dispatch() {
        for size in [0, MAX_JSON_BYTES as u32 + 1] {
            let mut header = &size.to_le_bytes()[..];
            assert!(read_request(&mut header).await.is_err());
        }
        let mut truncated = &[8u8, 0, 0, 0, b'{'][..];
        assert!(read_request(&mut truncated).await.is_err());
        let bytes =
            serde_json::to_vec(&json!({"id":9,"token":"test","command":"get_settings"})).unwrap();
        let mut frame = (bytes.len() as u32).to_le_bytes().to_vec();
        frame.extend(bytes);
        let parsed = read_request(&mut frame.as_slice()).await.unwrap();
        assert_eq!(parsed.id, 9);
        assert_eq!(parsed.args, json!({}));
    }
    #[test]
    fn arbitrary_parent_process_is_rejected() {
        assert!(validate_parent(0).is_err());
    }
}
