use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_MESSAGE_BYTES: usize = 32 * 1024;
const PARTY_STATUS_WRITE_INTERVAL: Duration = Duration::from_millis(500);
const PARTY_STATUS_ATTENTION_AFTER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupControlAction {
    Minimize,
    RestorePreset,
    GroupRaise,
    CloseSecondary,
}

impl GroupControlAction {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "minimize" => Some(Self::Minimize),
            "restore_preset" => Some(Self::RestorePreset),
            "group_raise" => Some(Self::GroupRaise),
            "close_secondary" => Some(Self::CloseSecondary),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::RestorePreset => "restore_preset",
            Self::GroupRaise => "group_raise",
            Self::CloseSecondary => "close_secondary",
        }
    }
}

#[derive(Debug, Clone)]
struct GroupControlWork {
    correlation_id: String,
    connection_id: u64,
    action: GroupControlAction,
}

#[derive(Debug, Clone, Copy)]
struct ConnectionIdentity {
    id: u64,
    handle: usize,
}

fn require_connection(session: &ExpectedSession, caller: ConnectionIdentity) -> Result<(), String> {
    if session.connected
        && session.connection_id == Some(caller.id)
        && session.pipe_handle == Some(caller.handle)
    {
        Ok(())
    } else {
        Err("message belongs to a stale broker connection".to_string())
    }
}

#[derive(Debug)]
struct GroupControlTarget {
    launch_session_id: String,
    account_id: String,
    order: u8,
    target: Result<BrokerWindowTarget, String>,
}

#[derive(Debug, Clone)]
pub struct BrokerBootstrap {
    pub pipe_name: String,
    pub launch_session_id: String,
    pub bootstrap_token: String,
    pub profile_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct BrokerClientStatus {
    character_name: String,
    hits: u32,
    hits_max: u32,
    mana: u32,
    mana_max: u32,
    stamina: u32,
    stamina_max: u32,
    weight: u32,
    weight_max: u32,
    backpack_items: Option<u32>,
    backpack_max: u32,
    poisoned: bool,
    paralyzed: bool,
    dead: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct PartyStatusMember {
    account_id: String,
    character_name: String,
    hits: u32,
    hits_max: u32,
    mana: u32,
    mana_max: u32,
    stamina: u32,
    stamina_max: u32,
    weight: u32,
    weight_max: u32,
    backpack_items: Option<u32>,
    backpack_max: u32,
    poisoned: bool,
    paralyzed: bool,
    dead: bool,
    attention: bool,
    #[serde(skip)]
    order: u8,
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
    hud_leader: bool,
    hud_order: u8,
    managed_z_order: Option<u8>,
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
    pending_group_control_result: Option<Value>,
    group_control_in_flight: bool,
    latest_status: Option<BrokerClientStatus>,
    last_status_received_at: Option<Instant>,
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
    pub fn live_secondary_accounts(
        &self,
        profile_id: &str,
    ) -> Result<Vec<(String, &'static str)>, String> {
        let sessions = self
            .shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        Ok(collect_profile_managed_group_targets(&sessions, profile_id)
            .into_iter()
            .map(|target| {
                (
                    target.account_id,
                    match target.order {
                        0 => "r0c0",
                        1 => "r0c1",
                        2 => "r1c0",
                        3 => "r1c1",
                        4 => "center",
                        _ => "managed",
                    },
                )
            })
            .collect())
    }

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
        hud_leader: bool,
        hud_order: u8,
        managed_z_order: Option<u8>,
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
                hud_leader,
                hud_order,
                managed_z_order,
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
                pending_group_control_result: None,
                group_control_in_flight: false,
                latest_status: None,
                last_status_received_at: None,
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
    let launch_session_id = (|| -> Result<(String, ConnectionIdentity), String> {
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
        session.pending_group_control_result = None;
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
                "negotiated_capabilities": [
                    "window_ready.v1",
                    "apply_managed_tile.v1",
                    "client_status.v1",
                    "party_status.v1",
                    "group_control.v1"
                ]
            }
        });
        drop(sessions);
        write_json_line(handle_value, &welcome)?;
        Ok((
            launch_session_id,
            ConnectionIdentity {
                id: connection_id,
                handle: handle_value,
            },
        ))
    })();

    if let Ok((launch_session_id, caller)) = &launch_session_id {
        let caller = *caller;
        let mut last_party_status_write = Instant::now() - PARTY_STATUS_WRITE_INTERVAL;
        loop {
            // The connection thread is the sole writer for this synchronous
            // pipe handle. Polling keeps it free to dispatch launcher commands
            // even when CE has no inbound heartbeat waiting to be read.
            if let Err(error) =
                write_pending_group_control_result(&shared, launch_session_id, handle_value)
            {
                set_session_error(&shared, launch_session_id, caller, error);
                break;
            }
            if let Err(error) = write_pending_command(&shared, launch_session_id, handle_value) {
                set_session_error(&shared, launch_session_id, caller, error);
                break;
            }
            if let Err(error) = write_party_status_if_due(
                &shared,
                launch_session_id,
                handle_value,
                &mut last_party_status_write,
            ) {
                set_session_error(&shared, launch_session_id, caller, error);
                break;
            }

            match reader.try_read_json_line() {
                Ok(Some(message)) => {
                    let result = match message.get("type").and_then(Value::as_str) {
                        Some("window_ready") => record_window_ready(
                            &shared,
                            launch_session_id,
                            caller,
                            client_pid,
                            &message,
                        ),
                        Some("client_status") => {
                            record_client_status(&shared, launch_session_id, caller, &message)
                        }
                        Some("group_control_request") => {
                            queue_group_control(&shared, launch_session_id, caller, &message)
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
                        set_session_error(&shared, launch_session_id, caller, error);
                        break;
                    }
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        if let Ok(mut sessions) = shared.sessions.lock() {
            if let Some(session) = sessions.get_mut(launch_session_id) {
                if require_connection(session, caller).is_ok() {
                    session.connected = false;
                    session.window_ready = false;
                    session.managed_tile_active = false;
                    session.pipe_handle = None;
                    session.latest_status = None;
                    session.last_status_received_at = None;
                    session.pending_group_control_result = None;
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
    caller: ConnectionIdentity,
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
    require_connection(session, caller)?;
    if let Some(previous) = session.hwnd_generation {
        if generation < previous || (generation == previous && session.hwnd != Some(hwnd_value)) {
            return Err("window_ready HWND generation regressed".to_string());
        }
    }
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

fn parse_client_status(message: &Value) -> Result<Option<BrokerClientStatus>, String> {
    let payload = message
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| "client_status payload is missing".to_string())?;
    let in_game = payload
        .get("in_game")
        .and_then(Value::as_bool)
        .ok_or_else(|| "client_status in_game is invalid".to_string())?;
    if !in_game {
        return Ok(None);
    }

    let character_name = required_text(payload, "character_name")?.trim();
    if character_name.is_empty() || character_name.chars().count() > 64 {
        return Err("client_status character_name is invalid".to_string());
    }

    fn bounded_u32(
        payload: &serde_json::Map<String, Value>,
        key: &str,
        maximum: u64,
    ) -> Result<u32, String> {
        let value = payload
            .get(key)
            .and_then(Value::as_u64)
            .filter(|value| *value <= maximum)
            .ok_or_else(|| format!("client_status {key} is invalid"))?;
        u32::try_from(value).map_err(|_| format!("client_status {key} is invalid"))
    }

    let backpack_items = match payload.get("backpack_items") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            u32::try_from(
                value
                    .as_u64()
                    .filter(|value| *value <= 10_000)
                    .ok_or_else(|| "client_status backpack_items is invalid".to_string())?,
            )
            .map_err(|_| "client_status backpack_items is invalid".to_string())?,
        ),
    };

    Ok(Some(BrokerClientStatus {
        character_name: character_name.to_string(),
        hits: bounded_u32(payload, "hits", 1_000_000)?,
        hits_max: bounded_u32(payload, "hits_max", 1_000_000)?,
        mana: bounded_u32(payload, "mana", 1_000_000)?,
        mana_max: bounded_u32(payload, "mana_max", 1_000_000)?,
        stamina: bounded_u32(payload, "stamina", 1_000_000)?,
        stamina_max: bounded_u32(payload, "stamina_max", 1_000_000)?,
        weight: bounded_u32(payload, "weight", 1_000_000)?,
        weight_max: bounded_u32(payload, "weight_max", 1_000_000)?,
        backpack_items,
        backpack_max: bounded_u32(payload, "backpack_max", 10_000)?,
        poisoned: payload
            .get("poisoned")
            .and_then(Value::as_bool)
            .ok_or_else(|| "client_status poisoned is invalid".to_string())?,
        paralyzed: payload
            .get("paralyzed")
            .and_then(Value::as_bool)
            .ok_or_else(|| "client_status paralyzed is invalid".to_string())?,
        dead: payload
            .get("dead")
            .and_then(Value::as_bool)
            .ok_or_else(|| "client_status dead is invalid".to_string())?,
    }))
}

fn record_client_status(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    caller: ConnectionIdentity,
    message: &Value,
) -> Result<(), String> {
    let status = parse_client_status(message)?;
    let mut sessions = shared
        .sessions
        .lock()
        .map_err(|_| "broker session lock is poisoned".to_string())?;
    let session = sessions
        .get_mut(launch_session_id)
        .ok_or_else(|| "launch session disappeared".to_string())?;
    require_connection(session, caller)?;
    session.latest_status = status;
    session.last_status_received_at = Some(Instant::now());
    session.last_error = None;
    Ok(())
}

fn begin_group_control(
    sessions: &mut HashMap<String, ExpectedSession>,
    launch_session_id: &str,
    message: &Value,
) -> Result<GroupControlWork, String> {
    let correlation_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .ok_or_else(|| "group control message_id is missing or invalid".to_string())?;
    let payload = message
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| "group control payload is missing".to_string())?;
    let action = payload
        .get("action")
        .and_then(Value::as_str)
        .and_then(GroupControlAction::parse)
        .ok_or_else(|| "group control action is unsupported".to_string())?;
    let session = sessions
        .get_mut(launch_session_id)
        .ok_or_else(|| "group control session disappeared".to_string())?;
    if !session.hud_leader {
        return Err("only the authenticated HUD leader can control the group".to_string());
    }
    if session.group_control_in_flight {
        return Err("a group control request is already in progress".to_string());
    }
    let connection_id = session
        .connection_id
        .ok_or_else(|| "group control connection identity is missing".to_string())?;
    session.group_control_in_flight = true;
    Ok(GroupControlWork {
        correlation_id: correlation_id.to_string(),
        connection_id,
        action,
    })
}

fn collect_managed_group_targets(
    sessions: &HashMap<String, ExpectedSession>,
    leader_session_id: &str,
) -> Result<Vec<GroupControlTarget>, String> {
    let leader = sessions
        .get(leader_session_id)
        .ok_or_else(|| "group control leader session disappeared".to_string())?;
    if !leader.hud_leader {
        return Err("only the authenticated HUD leader can control the group".to_string());
    }

    Ok(collect_profile_managed_group_targets(
        sessions,
        &leader.profile_id,
    ))
}

fn collect_profile_managed_group_targets(
    sessions: &HashMap<String, ExpectedSession>,
    profile_id: &str,
) -> Vec<GroupControlTarget> {
    let mut targets = sessions
        .iter()
        .filter(|(_, session)| {
            !session.hud_leader
                && session.profile_id == profile_id
                && session.connected
                && session.window_ready
                && session.managed_tile_active
        })
        .map(|(session_id, session)| {
            let target = (|| {
                Ok(BrokerWindowTarget {
                    pid: session
                        .expected_pid
                        .ok_or_else(|| "expected PID is missing".to_string())?,
                    process_creation_time: session
                        .expected_process_creation_time
                        .ok_or_else(|| "process creation identity is missing".to_string())?,
                    hwnd_value: session.hwnd.ok_or_else(|| "HWND is missing".to_string())?,
                    hwnd_generation: session
                        .hwnd_generation
                        .ok_or_else(|| "HWND generation is missing".to_string())?,
                })
            })();
            GroupControlTarget {
                launch_session_id: session_id.clone(),
                account_id: session.account_id.clone(),
                order: session.managed_z_order.unwrap_or(session.hud_order),
                target,
            }
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.account_id.cmp(&right.account_id))
    });
    targets
}

fn queue_group_control(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    caller: ConnectionIdentity,
    message: &Value,
) -> Result<(), String> {
    let work = {
        let mut sessions = shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        require_connection(
            sessions
                .get(launch_session_id)
                .ok_or_else(|| "launch session disappeared".to_string())?,
            caller,
        )?;
        begin_group_control(&mut sessions, launch_session_id, message)?
    };
    let worker_shared = Arc::clone(shared);
    let worker_session_id = launch_session_id.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name("ggo-group-control".to_string())
        .spawn(move || execute_group_control(worker_shared, worker_session_id, work))
    {
        if let Ok(mut sessions) = shared.sessions.lock() {
            if let Some(session) = sessions.get_mut(launch_session_id) {
                session.group_control_in_flight = false;
            }
        }
        return Err(format!("failed to start group control worker: {error}"));
    }
    Ok(())
}

fn execute_group_control(
    shared: Arc<BrokerShared>,
    leader_session_id: String,
    work: GroupControlWork,
) {
    let targets = shared
        .sessions
        .lock()
        .map_err(|_| "broker session lock is poisoned".to_string())
        .and_then(|sessions| collect_managed_group_targets(&sessions, &leader_session_id));

    let mut succeeded_count = 0usize;
    let mut failed_count = 0usize;
    let mut errors = Vec::new();
    let target_count = targets.as_ref().map_or(0, Vec::len);

    match targets {
        Ok(targets) => {
            for candidate in targets {
                let result =
                    candidate.target.and_then(|target| match work.action {
                        GroupControlAction::Minimize => {
                            super::minimize_broker_window(target).map(|_| ())
                        }
                        GroupControlAction::RestorePreset => {
                            super::restore_broker_window(target)?;
                            let mut sessions = shared
                                .sessions
                                .lock()
                                .map_err(|_| "broker session lock is poisoned".to_string())?;
                            let session =
                                sessions.get_mut(&candidate.launch_session_id).ok_or_else(
                                    || "managed session disappeared after restore".to_string(),
                                )?;
                            if !session.connected
                                || !session.managed_tile_active
                                || session.expected_pid != Some(target.pid)
                                || session.hwnd_generation != Some(target.hwnd_generation)
                            {
                                return Err("window identity changed during restore".to_string());
                            }
                            session.pending_apply_layout = true;
                            Ok(())
                        }
                        GroupControlAction::GroupRaise => {
                            super::raise_broker_window(target).map(|_| ())
                        }
                        GroupControlAction::CloseSecondary => {
                            super::close_broker_window(target).map(|_| ())
                        }
                    });

                match result {
                    Ok(()) => succeeded_count += 1,
                    Err(error) => {
                        failed_count += 1;
                        errors.push(format!("{}: {error}", candidate.account_id));
                    }
                }
            }
        }
        Err(error) => {
            failed_count = 1;
            errors.push(error);
        }
    }

    queue_group_control_result(
        &shared,
        &leader_session_id,
        &work,
        target_count,
        succeeded_count,
        failed_count,
        errors,
    );
}

fn queue_group_control_result(
    shared: &Arc<BrokerShared>,
    leader_session_id: &str,
    work: &GroupControlWork,
    target_count: usize,
    succeeded_count: usize,
    failed_count: usize,
    errors: Vec<String>,
) {
    let message = json!({
        "protocol": { "major": 1, "minor": 0 },
        "type": "group_control_result",
        "correlation_id": work.correlation_id,
        "message_id": format!("group-result-{}", super::launcher_observed_time_ms()),
        "payload": {
            "action": work.action.as_str(),
            "target_count": target_count,
            "succeeded_count": succeeded_count,
            "failed_count": failed_count,
            "errors": errors
        }
    });
    if let Ok(mut sessions) = shared.sessions.lock() {
        if let Some(session) = sessions.get_mut(leader_session_id) {
            session.group_control_in_flight = false;
            if session.connected && session.connection_id == Some(work.connection_id) {
                session.pending_group_control_result = Some(message);
            }
        }
    }
}

fn collect_party_status_members(
    sessions: &HashMap<String, ExpectedSession>,
    leader_session_id: &str,
    now: Instant,
) -> Result<Vec<PartyStatusMember>, String> {
    let leader = sessions
        .get(leader_session_id)
        .ok_or_else(|| "leader session disappeared".to_string())?;
    if !leader.hud_leader {
        return Ok(Vec::new());
    }

    let mut members = sessions
        .iter()
        .filter_map(|(session_id, session)| {
            if session_id == leader_session_id
                || session.profile_id != leader.profile_id
                || !session.connected
            {
                return None;
            }
            let status = session.latest_status.as_ref()?;
            let received_at = session.last_status_received_at?;
            let age = now.saturating_duration_since(received_at);
            Some(PartyStatusMember {
                account_id: session.account_id.clone(),
                character_name: status.character_name.clone(),
                hits: status.hits,
                hits_max: status.hits_max,
                mana: status.mana,
                mana_max: status.mana_max,
                stamina: status.stamina,
                stamina_max: status.stamina_max,
                weight: status.weight,
                weight_max: status.weight_max,
                backpack_items: status.backpack_items,
                backpack_max: status.backpack_max,
                poisoned: status.poisoned,
                paralyzed: status.paralyzed,
                dead: status.dead,
                attention: age >= PARTY_STATUS_ATTENTION_AFTER,
                order: session.hud_order,
            })
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.account_id.cmp(&right.account_id))
    });
    Ok(members)
}

#[cfg(windows)]
fn write_party_status_if_due(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    handle_value: usize,
    last_write: &mut Instant,
) -> Result<bool, String> {
    let now = Instant::now();
    if now.saturating_duration_since(*last_write) < PARTY_STATUS_WRITE_INTERVAL {
        return Ok(false);
    }

    let message = {
        let sessions = shared
            .sessions
            .lock()
            .map_err(|_| "broker session lock is poisoned".to_string())?;
        let session = sessions
            .get(launch_session_id)
            .ok_or_else(|| "launch session disappeared".to_string())?;
        if session.pipe_handle != Some(handle_value) {
            return Err("party status targeted a stale broker connection".to_string());
        }
        if !session.hud_leader {
            return Ok(false);
        }
        let members = collect_party_status_members(&sessions, launch_session_id, now)?;
        json!({
            "protocol": { "major": 1, "minor": 0 },
            "type": "party_status",
            "message_id": format!("party-{}", super::launcher_observed_time_ms()),
            "payload": { "members": members }
        })
    };

    write_json_line(handle_value, &message)?;
    *last_write = now;
    Ok(true)
}

fn set_session_error(
    shared: &Arc<BrokerShared>,
    launch_session_id: &str,
    caller: ConnectionIdentity,
    error: String,
) {
    if let Ok(mut sessions) = shared.sessions.lock() {
        if let Some(session) = sessions.get_mut(launch_session_id) {
            if require_connection(session, caller).is_ok() {
                session.last_error = Some(error);
            }
        }
    }
}

#[cfg(windows)]
fn write_pending_group_control_result(
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
            return Err("group result targeted a stale broker connection".to_string());
        }
        let Some(message) = session.pending_group_control_result.take() else {
            return Ok(false);
        };
        message
    };

    if let Err(error) = write_json_line(handle_value, &message) {
        if let Ok(mut sessions) = shared.sessions.lock() {
            if let Some(session) = sessions.get_mut(launch_session_id) {
                if session.pipe_handle == Some(handle_value)
                    && session.pending_group_control_result.is_none()
                {
                    session.pending_group_control_result = Some(message);
                }
            }
        }
        return Err(error);
    }
    Ok(true)
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
                hud_leader: true,
                hud_order: 0,
                managed_z_order: None,
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
                pending_group_control_result: None,
                group_control_in_flight: false,
                latest_status: None,
                last_status_received_at: None,
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

    fn client_status_message(in_game: bool) -> Value {
        json!({
            "type": "client_status",
            "payload": {
                "in_game": in_game,
                "character_name": "GGOImblue",
                "hits": 92,
                "hits_max": 100,
                "mana": 80,
                "mana_max": 100,
                "stamina": 75,
                "stamina_max": 100,
                "weight": 374,
                "weight_max": 456,
                "backpack_items": 34,
                "backpack_max": 125,
                "poisoned": false,
                "paralyzed": false,
                "dead": false
            }
        })
    }

    fn group_control_message(action: &str) -> Value {
        json!({
            "type": "group_control_request",
            "message_id": "ce-group-1",
            "payload": { "action": action }
        })
    }

    fn peer(account_id: &str, order: u8, status_at: Instant) -> ExpectedSession {
        let mut session = sessions().remove("session-a").unwrap();
        session.account_id = account_id.to_string();
        session.hud_leader = false;
        session.hud_order = order;
        session.connected = true;
        session.latest_status = parse_client_status(&client_status_message(true)).unwrap();
        session.last_status_received_at = Some(status_at);
        session
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
    fn replaced_connection_cannot_update_status_or_issue_commands_even_with_reused_handle() {
        let current = ConnectionIdentity { id: 8, handle: 123 };
        let old = ConnectionIdentity { id: 7, handle: 123 };
        let mut expected = sessions();
        let session = expected.get_mut("session-a").unwrap();
        session.connected = true;
        session.connection_id = Some(current.id);
        session.pipe_handle = Some(current.handle);
        let shared = Arc::new(BrokerShared {
            broker_instance_id: "test".to_string(),
            sessions: Mutex::new(expected),
            next_connection_id: AtomicU64::new(9),
        });
        record_client_status(&shared, "session-a", current, &client_status_message(true)).unwrap();
        assert!(
            record_client_status(&shared, "session-a", old, &client_status_message(false)).is_err()
        );
        assert!(queue_group_control(
            &shared,
            "session-a",
            old,
            &group_control_message("close_secondary")
        )
        .is_err());
        set_session_error(&shared, "session-a", old, "old error".to_string());
        let expected = shared.sessions.lock().unwrap();
        assert!(expected["session-a"].latest_status.is_some());
        assert!(!expected["session-a"].group_control_in_flight);
        assert!(expected["session-a"].last_error.is_none());
        assert!(require_connection(
            &expected["session-a"],
            ConnectionIdentity { id: 8, handle: 999 }
        )
        .is_err());
    }

    #[test]
    fn client_status_uses_ingame_payload_and_out_of_game_clears_it() {
        let status = parse_client_status(&client_status_message(true))
            .unwrap()
            .unwrap();
        assert_eq!(status.character_name, "GGOImblue");
        assert_eq!(status.backpack_items, Some(34));
        assert!(parse_client_status(&client_status_message(false))
            .unwrap()
            .is_none());

        let mut invalid = client_status_message(true);
        invalid["payload"]["hits"] = json!(1_000_001);
        assert!(parse_client_status(&invalid).unwrap_err().contains("hits"));
    }

    #[test]
    fn party_status_excludes_leader_orders_peers_and_marks_only_stale_rows() {
        let now = Instant::now();
        let mut expected = sessions();
        {
            let leader = expected.get_mut("session-a").unwrap();
            leader.connected = true;
            leader.latest_status = parse_client_status(&client_status_message(true)).unwrap();
            leader.last_status_received_at = Some(now);
        }
        expected.insert(
            "session-late".to_string(),
            peer("account-late", 2, now - Duration::from_secs(4)),
        );
        expected.insert(
            "session-first".to_string(),
            peer("account-first", 1, now - Duration::from_secs(1)),
        );
        expected.insert(
            "session-expired".to_string(),
            peer("account-expired", 3, now - Duration::from_secs(10)),
        );

        let members = collect_party_status_members(&expected, "session-a", now).unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].account_id, "account-first");
        assert!(!members[0].attention);
        assert_eq!(members[1].account_id, "account-late");
        assert!(members[1].attention);
        assert_eq!(members[2].account_id, "account-expired");
        assert!(members[2].attention);
    }

    #[test]
    fn group_control_requires_the_leader_and_allows_only_group_window_actions() {
        let mut expected = sessions();
        {
            let leader = expected.get_mut("session-a").unwrap();
            leader.connected = true;
            leader.connection_id = Some(7);
        }
        let work = begin_group_control(
            &mut expected,
            "session-a",
            &group_control_message("restore_preset"),
        )
        .unwrap();
        assert_eq!(work.action, GroupControlAction::RestorePreset);
        assert_eq!(work.connection_id, 7);
        assert!(expected["session-a"].group_control_in_flight);
        assert_eq!(
            GroupControlAction::parse("group_raise"),
            Some(GroupControlAction::GroupRaise)
        );
        assert_eq!(
            GroupControlAction::parse("close_secondary"),
            Some(GroupControlAction::CloseSecondary)
        );

        let mut nonleader = sessions();
        {
            let session = nonleader.get_mut("session-a").unwrap();
            session.hud_leader = false;
            session.connected = true;
            session.connection_id = Some(8);
        }
        assert!(begin_group_control(
            &mut nonleader,
            "session-a",
            &group_control_message("minimize")
        )
        .unwrap_err()
        .contains("leader"));

        let mut invalid = sessions();
        assert!(
            begin_group_control(&mut invalid, "session-a", &group_control_message("close"))
                .unwrap_err()
                .contains("unsupported")
        );
    }

    #[test]
    fn group_control_targets_only_active_managed_peers_in_the_leader_profile() {
        let mut expected = sessions();
        {
            let leader = expected.get_mut("session-a").unwrap();
            leader.window_ready = true;
            leader.managed_tile_active = true;
        }

        let mut managed = peer("managed", 2, Instant::now());
        managed.window_ready = true;
        managed.managed_tile_active = true;
        managed.managed_z_order = Some(4);
        managed.expected_pid = Some(22);
        managed.expected_process_creation_time = Some(2200);
        managed.hwnd = Some(222);
        managed.hwnd_generation = Some(2);
        expected.insert("session-managed".to_string(), managed);

        let mut broken_managed = peer("broken-managed", 3, Instant::now());
        broken_managed.window_ready = true;
        broken_managed.managed_tile_active = true;
        broken_managed.managed_z_order = Some(0);
        broken_managed.expected_pid = Some(23);
        expected.insert("session-broken-managed".to_string(), broken_managed);

        let mut bard = peer("bard", 1, Instant::now());
        bard.window_ready = true;
        bard.managed_tile_active = false;
        expected.insert("session-bard".to_string(), bard);

        let mut other_profile = peer("other-profile", 4, Instant::now());
        other_profile.profile_id = "profile-b".to_string();
        other_profile.window_ready = true;
        other_profile.managed_tile_active = true;
        expected.insert("session-other-profile".to_string(), other_profile);

        let mut stopped = peer("stopped", 5, Instant::now());
        stopped.connected = false;
        stopped.window_ready = true;
        stopped.managed_tile_active = true;
        expected.insert("session-stopped".to_string(), stopped);

        let targets = collect_managed_group_targets(&expected, "session-a").unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].account_id, "broken-managed");
        assert!(targets[0].target.is_err());
        assert_eq!(targets[1].account_id, "managed");
        assert!(targets[1].target.is_ok());
        let launcher_targets = collect_profile_managed_group_targets(&expected, "profile-a");
        assert_eq!(
            launcher_targets
                .iter()
                .map(|t| &t.account_id)
                .collect::<Vec<_>>(),
            targets.iter().map(|t| &t.account_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn group_control_result_is_a_safe_noop_and_never_crosses_reconnections() {
        let mut expected = sessions();
        {
            let leader = expected.get_mut("session-a").unwrap();
            leader.connected = true;
            leader.connection_id = Some(7);
            leader.group_control_in_flight = true;
        }
        let shared = Arc::new(BrokerShared {
            broker_instance_id: "test-broker".to_string(),
            sessions: Mutex::new(expected),
            next_connection_id: AtomicU64::new(1),
        });
        let work = GroupControlWork {
            correlation_id: "ce-group-1".to_string(),
            connection_id: 7,
            action: GroupControlAction::Minimize,
        };

        queue_group_control_result(&shared, "session-a", &work, 0, 0, 0, Vec::new());
        {
            let sessions = shared.sessions.lock().unwrap();
            let leader = &sessions["session-a"];
            assert!(!leader.group_control_in_flight);
            let result = leader.pending_group_control_result.as_ref().unwrap();
            assert_eq!(result["payload"]["target_count"], 0);
            assert_eq!(result["payload"]["failed_count"], 0);
        }

        {
            let mut sessions = shared.sessions.lock().unwrap();
            let leader = sessions.get_mut("session-a").unwrap();
            leader.connection_id = Some(8);
            leader.group_control_in_flight = true;
            leader.pending_group_control_result = None;
        }
        queue_group_control_result(&shared, "session-a", &work, 1, 1, 0, Vec::new());

        let sessions = shared.sessions.lock().unwrap();
        let leader = &sessions["session-a"];
        assert!(!leader.group_control_in_flight);
        assert!(leader.pending_group_control_result.is_none());
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
