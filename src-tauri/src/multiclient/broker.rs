use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_MESSAGE_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone)]
pub struct BrokerBootstrap {
    pub pipe_name: String,
    pub launch_session_id: String,
    pub bootstrap_token: String,
    pub profile_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrokerSessionDiagnostic {
    pub launch_session_id: String,
    pub profile_id: String,
    pub account_id: String,
    pub expected_pid: Option<u32>,
    pub connected: bool,
    pub window_ready: bool,
    pub managed_tile_active: bool,
    pub hwnd: Option<String>,
    pub hwnd_generation: Option<u64>,
    pub reconnect_count: u64,
    pub pending_apply_layout: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct BrokerWindowTarget {
    pub pid: u32,
    pub process_creation_time: u64,
    pub hwnd_value: usize,
    pub hwnd_generation: u64,
}

#[derive(Debug, Clone)]
struct ExpectedSession {
    profile_id: String,
    account_id: String,
    bootstrap_token: String,
    expected_pid: Option<u32>,
    expected_process_creation_time: Option<u64>,
    connected: bool,
    window_ready: bool,
    managed_tile_active: bool,
    hwnd: Option<usize>,
    hwnd_generation: Option<u64>,
    reconnect_count: u64,
    connection_id: Option<u64>,
    pipe_handle: Option<usize>,
    pending_apply_layout: bool,
    last_error: Option<String>,
}

struct BrokerShared {
    broker_instance_id: String,
    sessions: Mutex<HashMap<String, ExpectedSession>>,
    next_connection_id: AtomicU64,
}

pub struct BrokerServer {
    pipe_name: String,
    shared: Arc<BrokerShared>,
}

impl BrokerServer {
    pub fn start() -> Self {
        let broker_nonce = random_hex(16).unwrap_or_else(|_| {
            format!(
                "{:x}{:x}",
                std::process::id(),
                super::launcher_observed_time_ms()
            )
        });
        let pipe_name = format!("ggo-launcher-{}-{broker_nonce}", std::process::id());
        let shared = Arc::new(BrokerShared {
            broker_instance_id: random_hex(16).unwrap_or_else(|_| broker_nonce.clone()),
            sessions: Mutex::new(HashMap::new()),
            next_connection_id: AtomicU64::new(1),
        });

        #[cfg(windows)]
        {
            let server_shared = Arc::clone(&shared);
            let server_pipe_name = pipe_name.clone();
            std::thread::Builder::new()
                .name("ggo-broker-accept".to_string())
                .spawn(move || run_server(server_pipe_name, server_shared))
                .expect("failed to start GGO broker thread");
        }

        Self { pipe_name, shared }
    }

    pub fn prepare_session(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<BrokerBootstrap, String> {
        let launch_session_id = random_hex(16)?;
        let bootstrap_token = random_hex(32)?;
        let mut sessions = self
            .shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        sessions.retain(|_, session| {
            session.profile_id != profile_id || session.account_id != account_id
        });
        sessions.insert(
            launch_session_id.clone(),
            ExpectedSession {
                profile_id: profile_id.to_string(),
                account_id: account_id.to_string(),
                bootstrap_token: bootstrap_token.clone(),
                expected_pid: None,
                expected_process_creation_time: None,
                connected: false,
                window_ready: false,
                managed_tile_active: false,
                hwnd: None,
                hwnd_generation: None,
                reconnect_count: 0,
                connection_id: None,
                pipe_handle: None,
                pending_apply_layout: false,
                last_error: None,
            },
        );
        Ok(BrokerBootstrap {
            pipe_name: self.pipe_name.clone(),
            launch_session_id,
            bootstrap_token,
            profile_id: profile_id.to_string(),
            account_id: account_id.to_string(),
        })
    }

    pub fn bind_process(
        &self,
        launch_session_id: &str,
        pid: u32,
        process_creation_time: Option<u64>,
    ) -> Result<(), String> {
        let mut sessions = self
            .shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let session = sessions
            .get_mut(launch_session_id)
            .ok_or_else(|| "broker launch session was not prepared".to_string())?;
        session.expected_pid = Some(pid);
        session.expected_process_creation_time = process_creation_time;
        Ok(())
    }

    pub fn cancel_session(&self, launch_session_id: &str) {
        if let Ok(mut sessions) = self.shared.sessions.lock() {
            sessions.remove(launch_session_id);
        }
    }

    pub fn diagnostics(&self) -> Vec<BrokerSessionDiagnostic> {
        let Ok(sessions) = self.shared.sessions.lock() else {
            return Vec::new();
        };
        let mut result = sessions
            .iter()
            .map(|(launch_session_id, session)| BrokerSessionDiagnostic {
                launch_session_id: launch_session_id.clone(),
                profile_id: session.profile_id.clone(),
                account_id: session.account_id.clone(),
                expected_pid: session.expected_pid,
                connected: session.connected,
                window_ready: session.window_ready,
                managed_tile_active: session.managed_tile_active,
                hwnd: session.hwnd.map(|value| format!("0x{value:X}")),
                hwnd_generation: session.hwnd_generation,
                reconnect_count: session.reconnect_count,
                pending_apply_layout: session.pending_apply_layout,
                last_error: session.last_error.clone(),
            })
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left.launch_session_id.cmp(&right.launch_session_id));
        result
    }

    pub fn request_apply_layout(&self, profile_id: &str, account_id: &str) -> Result<(), String> {
        let mut sessions = self
            .shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let session = sessions
            .values_mut()
            .find(|session| {
                session.profile_id == profile_id
                    && session.account_id == account_id
                    && session.connected
            })
            .ok_or_else(|| format!("{account_id}: CE broker connection is not ready"))?;
        // Never write from the Tauri command thread. A synchronous named-pipe
        // handle may already be blocked in ReadFile on its connection thread;
        // attempting WriteFile here can wait behind that read and freeze the UI.
        // The owning connection thread consumes this flag on the next heartbeat.
        session.pending_apply_layout = true;
        eprintln!(
            "[ggo-broker] queued apply_managed_tile profile={} account={}",
            session.profile_id, session.account_id
        );
        Ok(())
    }

    pub fn window_target(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<BrokerWindowTarget, String> {
        let sessions = self
            .shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let session = sessions
            .values()
            .find(|session| {
                session.profile_id == profile_id
                    && session.account_id == account_id
                    && session.connected
                    && session.window_ready
            })
            .ok_or_else(|| format!("{account_id}: window_ready is pending"))?;
        if !session.managed_tile_active {
            return Err(format!("{account_id}: unmanaged for this launcher run"));
        }
        Ok(BrokerWindowTarget {
            pid: session
                .expected_pid
                .ok_or_else(|| format!("{account_id}: expected PID is missing"))?,
            process_creation_time: session
                .expected_process_creation_time
                .ok_or_else(|| format!("{account_id}: process creation identity is missing"))?,
            hwnd_value: session
                .hwnd
                .ok_or_else(|| format!("{account_id}: HWND is missing"))?,
            hwnd_generation: session
                .hwnd_generation
                .ok_or_else(|| format!("{account_id}: HWND generation is missing"))?,
        })
    }

    /// Stage diagnostics only: sever one authenticated pipe so the running CE
    /// must reconnect to the same broker with the same launch identity.
    pub fn disconnect_session_for_test(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<(), String> {
        let handle_value = {
            let sessions = self
                .shared
                .sessions
                .lock()
                .map_err(|_| "broker session lock is poisoned".to_string())?;
            sessions
                .values()
                .find(|session| {
                    session.profile_id == profile_id
                        && session.account_id == account_id
                        && session.connected
                })
                .and_then(|session| session.pipe_handle)
                .ok_or_else(|| format!("{account_id}: connected broker session not found"))?
        };

        disconnect_pipe_for_test(handle_value)
    }
}

#[cfg(windows)]
fn disconnect_pipe_for_test(handle_value: usize) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
    use windows_sys::Win32::System::Pipes::DisconnectNamedPipe;
    if unsafe { DisconnectNamedPipe(handle_value as HANDLE) } == 0 {
        return Err(format!(
            "DisconnectNamedPipe failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn disconnect_pipe_for_test(_handle_value: usize) -> Result<(), String> {
    Err("broker reconnect diagnostics require Windows".to_string())
}

fn validate_hello(
    sessions: &mut HashMap<String, ExpectedSession>,
    client_pid: u32,
    message: &Value,
) -> Result<String, String> {
    if message.get("type").and_then(Value::as_str) != Some("hello") {
        return Err("first broker message must be hello".to_string());
    }
    let payload = message
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| "hello payload is missing".to_string())?;
    let launch_session_id = required_text(payload, "launch_session_id")?;
    let profile_id = required_text(payload, "profile_id")?;
    let account_id = required_text(payload, "account_id")?;
    let token = required_text(payload, "bootstrap_token")?;
    let session = sessions
        .get_mut(launch_session_id)
        .ok_or_else(|| "unknown launch_session_id".to_string())?;
    if session.profile_id != profile_id || session.account_id != account_id {
        return Err("profile/account identity mismatch".to_string());
    }
    if !constant_time_eq(session.bootstrap_token.as_bytes(), token.as_bytes()) {
        return Err("bootstrap token mismatch".to_string());
    }
    if session.expected_pid != Some(client_pid) {
        return Err(format!(
            "pipe client PID mismatch: expected {:?}, observed {client_pid}",
            session.expected_pid
        ));
    }
    if let Some(expected_creation) = session.expected_process_creation_time {
        let observed_creation = super::windows::query_process_creation_time(client_pid)?;
        if observed_creation != expected_creation {
            return Err("pipe client process creation time mismatch".to_string());
        }
    }
    Ok(launch_session_id.to_string())
}

fn required_text<'a>(
    payload: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .ok_or_else(|| format!("hello field {key} is missing or invalid"))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(windows)]
fn random_hex(byte_count: usize) -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let mut bytes = vec![0u8; byte_count];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            u32::try_from(bytes.len()).map_err(|_| "random request is too large")?,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(format!("BCryptGenRandom failed with NTSTATUS {status}"));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(not(windows))]
fn random_hex(byte_count: usize) -> Result<String, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seed = COUNTER.fetch_add(1, Ordering::Relaxed) ^ super::launcher_observed_time_ms();
    Ok((0..byte_count)
        .map(|index| format!("{:02x}", seed.wrapping_add(index as u64) as u8))
        .collect())
}

#[cfg(windows)]
fn run_server(pipe_name: String, shared: Arc<BrokerShared>) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let full_name = format!(r"\\.\pipe\{pipe_name}");
    let wide_name = std::ffi::OsStr::new(&full_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();

    loop {
        let (security_descriptor, security_attributes) = match pipe_security_attributes() {
            Ok(value) => value,
            Err(error) => {
                eprintln!("[ggo-broker] secure pipe descriptor failed: {error}");
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }
        };
        let handle = unsafe {
            CreateNamedPipeW(
                wide_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                MAX_MESSAGE_BYTES as u32,
                MAX_MESSAGE_BYTES as u32,
                1000,
                &security_attributes,
            )
        };
        drop(security_descriptor);
        if handle == INVALID_HANDLE_VALUE {
            eprintln!("[ggo-broker] CreateNamedPipeW failed: {}", unsafe {
                GetLastError()
            });
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }

        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED;
        if !connected {
            unsafe { CloseHandle(handle) };
            continue;
        }

        let connection_shared = Arc::clone(&shared);
        let handle_value = handle as usize;
        std::thread::Builder::new()
            .name("ggo-broker-client".to_string())
            .spawn(move || handle_client(handle_value, connection_shared))
            .ok();
    }
}

#[cfg(windows)]
struct OwnedSecurityDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}

#[cfg(windows)]
fn pipe_security_attributes() -> Result<
    (
        OwnedSecurityDescriptor,
        windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
    ),
    String,
> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    // Protected DACL: LocalSystem, administrators and the object owner only.
    // Combined with PIPE_REJECT_REMOTE_CLIENTS this excludes remote and other
    // interactive users before the per-launch token/PID checks run.
    let sddl = std::ffi::OsStr::new("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(format!(
            "ConvertStringSecurityDescriptor failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    Ok((
        OwnedSecurityDescriptor(descriptor),
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        },
    ))
}

#[cfg(windows)]
fn handle_client(handle_value: usize, shared: Arc<BrokerShared>) {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Pipes::{DisconnectNamedPipe, GetNamedPipeClientProcessId};
    let handle = handle_value as HANDLE;
    let mut client_pid = 0u32;
    if unsafe { GetNamedPipeClientProcessId(handle, &mut client_pid) } == 0 {
        unsafe {
            DisconnectNamedPipe(handle);
            CloseHandle(handle);
        }
        return;
    }

    let mut reader = PipeLineReader::new(handle_value);
    let launch_session_id = (|| -> Result<String, String> {
        let hello = reader.read_json_line()?;
        let mut sessions = shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let launch_session_id = validate_hello(&mut sessions, client_pid, &hello)?;
        let connection_id = shared.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let session = sessions
            .get_mut(&launch_session_id)
            .ok_or_else(|| "launch session disappeared".to_string())?;
        session.connected = true;
        session.window_ready = false;
        session.managed_tile_active = false;
        session.reconnect_count += 1;
        session.connection_id = Some(connection_id);
        session.pipe_handle = Some(handle_value);
        session.last_error = None;
        eprintln!(
            "[ggo-broker] authenticated profile={} account={} pid={} reconnect={}",
            session.profile_id, session.account_id, client_pid, session.reconnect_count
        );
        let welcome = json!({
            "protocol": { "major": 1, "minor": 0 },
            "type": "welcome",
            "correlation_id": hello.get("message_id").and_then(Value::as_str).unwrap_or(""),
            "payload": {
                "broker_instance_id": shared.broker_instance_id,
                "broker_pid": std::process::id(),
                "heartbeat_interval_ms": 1000,
                "negotiated_capabilities": ["window_ready.v1", "apply_managed_tile.v1"]
            }
        });
        drop(sessions);
        write_json_line(handle_value, &welcome)?;
        Ok(launch_session_id)
    })();

    if let Ok(launch_session_id) = &launch_session_id {
        loop {
            // The connection thread is the sole writer for this synchronous
            // pipe handle. Polling keeps it free to dispatch launcher commands
            // even when CE has no inbound heartbeat waiting to be read.
            if let Err(error) = write_pending_command(&shared, launch_session_id, handle_value) {
                set_session_error(&shared, launch_session_id, error);
                break;
            }

            match reader.try_read_json_line() {
                Ok(Some(message)) => {
                    let result = match message.get("type").and_then(Value::as_str) {
                        Some("window_ready") => {
                            record_window_ready(&shared, launch_session_id, client_pid, &message)
                        }
                        Some("ping") => {
                            write_json_line(handle_value, &json!({ "type": "pong", "payload": {} }))
                        }
                        _ => Err("unsupported broker message type".to_string()),
                    };
                    if let Err(error) = result {
                        eprintln!(
                            "[ggo-broker] rejected message session={} pid={}: {}",
                            launch_session_id, client_pid, error
                        );
                        set_session_error(&shared, launch_session_id, error);
                        break;
                    }
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        if let Ok(mut sessions) = shared.sessions.lock() {
            if let Some(session) = sessions.get_mut(launch_session_id) {
                if session.pipe_handle == Some(handle_value) {
                    session.connected = false;
                    session.window_ready = false;
                    session.managed_tile_active = false;
                    session.pipe_handle = None;
                }
            }
        }
    }

    unsafe {
        DisconnectNamedPipe(handle);
        CloseHandle(handle);
    }
}

#[cfg(windows)]
fn record_window_ready(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    client_pid: u32,
    message: &Value,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let payload = message
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| "window_ready payload is missing".to_string())?;
    let hwnd_text = required_text(payload, "hwnd_u64")?;
    let hwnd_value = hwnd_text
        .parse::<usize>()
        .map_err(|_| "window_ready hwnd_u64 is invalid".to_string())?;
    let generation = payload
        .get("hwnd_generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| "window_ready hwnd_generation is invalid".to_string())?;
    let managed_tile_active = payload
        .get("managed_tile_active")
        .and_then(Value::as_bool)
        .ok_or_else(|| "window_ready managed_tile_active is invalid".to_string())?;
    let mut owner_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd_value as HWND, &mut owner_pid) };
    if owner_pid != client_pid {
        return Err(format!(
            "window_ready HWND owner mismatch: expected {client_pid}, observed {owner_pid}"
        ));
    }
    let mut sessions = shared
        .sessions
        .lock()
        .map_err(|_| "broker session lock is poisoned".to_string())?;
    let session = sessions
        .get_mut(launch_session_id)
        .ok_or_else(|| "launch session disappeared".to_string())?;
    session.hwnd = Some(hwnd_value);
    session.hwnd_generation = Some(generation);
    session.window_ready = true;
    session.managed_tile_active = managed_tile_active;
    session.last_error = None;
    eprintln!(
        "[ggo-broker] window_ready profile={} account={} pid={} hwnd=0x{:X} generation={} managed={}",
        session.profile_id,
        session.account_id,
        client_pid,
        hwnd_value,
        generation,
        managed_tile_active
    );
    Ok(())
}

fn set_session_error(shared: &Arc<BrokerShared>, launch_session_id: &str, error: String) {
    if let Ok(mut sessions) = shared.sessions.lock() {
        if let Some(session) = sessions.get_mut(launch_session_id) {
            session.last_error = Some(error);
        }
    }
}

#[cfg(windows)]
fn write_pending_command(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    handle_value: usize,
) -> Result<bool, String> {
    let message = {
        let mut sessions = shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let session = sessions
            .get_mut(launch_session_id)
            .ok_or_else(|| "launch session disappeared".to_string())?;
        if session.pipe_handle != Some(handle_value) {
            return Err("pending command targeted a stale broker connection".to_string());
        }
        if !session.pending_apply_layout {
            return Ok(false);
        }
        let connection_id = session.connection_id.unwrap_or_default();
        session.pending_apply_layout = false;
        json!({
            "protocol": { "major": 1, "minor": 0 },
            "type": "apply_managed_tile",
            "message_id": format!("cmd-{connection_id}-{}", super::launcher_observed_time_ms()),
            "payload": {}
        })
    };

    eprintln!("[ggo-broker] dispatching apply_managed_tile session={launch_session_id}");
    let result = write_json_line(handle_value, &message);
    if result.is_ok() {
        eprintln!("[ggo-broker] dispatched apply_managed_tile session={launch_session_id}");
    }
    if result.is_err() {
        if let Ok(mut sessions) = shared.sessions.lock() {
            if let Some(session) = sessions.get_mut(launch_session_id) {
                session.pending_apply_layout = true;
            }
        }
    }
    result.map(|_| true)
}

#[cfg(windows)]
struct PipeLineReader {
    handle_value: usize,
    pending: Vec<u8>,
}

#[cfg(windows)]
impl PipeLineReader {
    fn new(handle_value: usize) -> Self {
        Self {
            handle_value,
            pending: Vec::with_capacity(1024),
        }
    }

    fn read_json_line(&mut self) -> Result<Value, String> {
        use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
        use windows_sys::Win32::Storage::FileSystem::ReadFile;
        let handle = self.handle_value as HANDLE;
        let mut buffer = [0u8; 1024];
        loop {
            if let Some(message) = self.take_pending_json_line()? {
                return Ok(message);
            }
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                return Err(format!("pipe read failed: {}", unsafe { GetLastError() }));
            }
            self.pending.extend_from_slice(&buffer[..read as usize]);
            if self.pending.len() > MAX_MESSAGE_BYTES {
                return Err("broker message exceeded 32 KiB".to_string());
            }
        }
    }

    fn try_read_json_line(&mut self) -> Result<Option<Value>, String> {
        use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
        use windows_sys::Win32::Storage::FileSystem::ReadFile;
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;

        if let Some(message) = self.take_pending_json_line()? {
            return Ok(Some(message));
        }

        let handle = self.handle_value as HANDLE;
        let mut available = 0u32;
        if unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!("pipe peek failed: {}", unsafe { GetLastError() }));
        }
        if available == 0 {
            return Ok(None);
        }

        let mut buffer = [0u8; 1024];
        let requested = available.min(buffer.len() as u32);
        let mut read = 0u32;
        if unsafe {
            ReadFile(
                handle,
                buffer.as_mut_ptr(),
                requested,
                &mut read,
                std::ptr::null_mut(),
            )
        } == 0
            || read == 0
        {
            return Err(format!("pipe read failed: {}", unsafe { GetLastError() }));
        }
        self.pending.extend_from_slice(&buffer[..read as usize]);
        if self.pending.len() > MAX_MESSAGE_BYTES {
            return Err("broker message exceeded 32 KiB".to_string());
        }
        self.take_pending_json_line()
    }

    fn take_pending_json_line(&mut self) -> Result<Option<Value>, String> {
        let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') else {
            return Ok(None);
        };
        let line = self.pending.drain(..=newline).collect::<Vec<_>>();
        serde_json::from_slice(&line[..line.len() - 1])
            .map(Some)
            .map_err(|error| format!("invalid broker JSON: {error}"))
    }
}

#[cfg(windows)]
fn write_json_line(handle_value: usize, message: &Value) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    let mut bytes = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut written = 0u32;
        let ok = unsafe {
            WriteFile(
                handle_value as HANDLE,
                bytes[offset..].as_ptr(),
                u32::try_from(bytes.len() - offset).map_err(|_| "broker message is too large")?,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            return Err(format!("pipe write failed: {}", unsafe { GetLastError() }));
        }
        offset += written as usize;
    }
    Ok(())
}

#[cfg(not(windows))]
fn write_json_line(_handle_value: usize, _message: &Value) -> Result<(), String> {
    Err("GGO broker is available only on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessions() -> HashMap<String, ExpectedSession> {
        HashMap::from([(
            "session-a".to_string(),
            ExpectedSession {
                profile_id: "profile-a".to_string(),
                account_id: "account-a".to_string(),
                bootstrap_token: "secret-token".to_string(),
                expected_pid: Some(std::process::id()),
                expected_process_creation_time: None,
                connected: false,
                window_ready: false,
                managed_tile_active: false,
                hwnd: None,
                hwnd_generation: None,
                reconnect_count: 0,
                connection_id: None,
                pipe_handle: None,
                pending_apply_layout: false,
                last_error: None,
            },
        )])
    }

    fn hello(token: &str) -> Value {
        json!({
            "type": "hello",
            "payload": {
                "launch_session_id": "session-a",
                "profile_id": "profile-a",
                "account_id": "account-a",
                "bootstrap_token": token
            }
        })
    }

    #[test]
    fn hello_requires_matching_token_and_os_pipe_pid() {
        let pid = std::process::id();
        assert_eq!(
            validate_hello(&mut sessions(), pid, &hello("secret-token")).unwrap(),
            "session-a"
        );
        assert!(validate_hello(&mut sessions(), pid, &hello("wrong-token"))
            .unwrap_err()
            .contains("token"));
        assert!(validate_hello(
            &mut sessions(),
            pid.saturating_add(1),
            &hello("secret-token")
        )
        .unwrap_err()
        .contains("PID"));
    }

    #[test]
    fn token_comparison_does_not_accept_prefixes() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
        assert!(!constant_time_eq(b"same", b"diff"));
    }

    #[test]
    fn layout_request_is_queued_without_writing_from_the_caller() {
        let mut expected = sessions();
        let session = expected.get_mut("session-a").unwrap();
        session.connected = true;
        session.pipe_handle = Some(123);
        let server = BrokerServer {
            pipe_name: "test-only".to_string(),
            shared: Arc::new(BrokerShared {
                broker_instance_id: "test-broker".to_string(),
                sessions: Mutex::new(expected),
                next_connection_id: AtomicU64::new(1),
            }),
        };

        server
            .request_apply_layout("profile-a", "account-a")
            .unwrap();

        let sessions = server.shared.sessions.lock().unwrap();
        assert!(sessions["session-a"].pending_apply_layout);
    }

    #[cfg(windows)]
    #[test]
    fn polling_reader_preserves_and_drains_complete_buffered_lines() {
        let mut reader = PipeLineReader::new(0);
        reader
            .pending
            .extend_from_slice(b"{\"type\":\"ping\"}\n{\"type\":\"window_ready\"}\n");

        assert_eq!(
            reader
                .take_pending_json_line()
                .unwrap()
                .unwrap()
                .get("type")
                .and_then(Value::as_str),
            Some("ping")
        );
        assert_eq!(
            reader
                .take_pending_json_line()
                .unwrap()
                .unwrap()
                .get("type")
                .and_then(Value::as_str),
            Some("window_ready")
        );
        assert!(reader.take_pending_json_line().unwrap().is_none());
    }
}
