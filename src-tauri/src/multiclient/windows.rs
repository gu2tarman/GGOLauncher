use super::layout::SignedRect;
use super::BrokerWindowTarget;
use serde::Serialize;

pub const GAME_WINDOW_CLASS: &str = "SDL_app";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonitorDescriptor {
    pub device_name: String,
    pub is_primary: bool,
    pub monitor_rect: SignedRect,
    pub work_area: SignedRect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonitorEnumeration {
    pub monitors: Vec<MonitorDescriptor>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowCandidateDiagnostic {
    pub hwnd: String,
    pub class_name: String,
    pub title: String,
    pub is_visible: bool,
    pub has_owner: bool,
    pub eligible_game_window: bool,
    pub exclusion_reason: Option<&'static str>,
    pub show_cmd: Option<u32>,
    pub is_minimized: Option<bool>,
    pub dpi: Option<u32>,
    pub style: String,
    pub ex_style: String,
    pub window_rect: Option<SignedRect>,
    pub normal_rect: Option<SignedRect>,
    pub client_rect_screen: Option<SignedRect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcessWindowInspection {
    pub pid: u32,
    /// Decimal FILETIME ticks, serialized as text to avoid JavaScript u64 precision loss.
    pub expected_process_creation_time_filetime_100ns: Option<String>,
    pub observed_process_creation_time_filetime_100ns: Option<String>,
    pub process_creation_time_matches: Option<bool>,
    pub selected_hwnd: Option<String>,
    pub management_eligible: bool,
    pub status: &'static str,
    pub candidates: Vec<WindowCandidateDiagnostic>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub(crate) selected_hwnd_value: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedWindowPlacement {
    pub pid: u32,
    pub process_creation_time_filetime_100ns: u64,
    pub hwnd_value: usize,
    pub flags: u32,
    pub show_cmd: u32,
    pub min_position_x: i32,
    pub min_position_y: i32,
    pub max_position_x: i32,
    pub max_position_y: i32,
    pub normal_rect: SignedRect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowActionResult {
    pub action: &'static str,
    pub pid: u32,
    pub hwnd: String,
    pub requested_outer_rect: Option<SignedRect>,
    pub actual_window_rect: Option<SignedRect>,
    pub actual_normal_rect: Option<SignedRect>,
    pub actual_client_rect_screen: Option<SignedRect>,
    pub requested_outer_rect_matches: Option<bool>,
    pub requested_position_matches: Option<bool>,
    pub show_cmd: Option<u32>,
    pub is_minimized: Option<bool>,
    pub dpi: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowControlObservation {
    pub action: &'static str,
    pub pid: u32,
    pub hwnd: String,
    pub hwnd_generation: u64,
    pub is_minimized: Option<bool>,
    pub dpi: Option<u32>,
}

fn game_window_exclusion_reason(class_name: &str, has_owner: bool) -> Option<&'static str> {
    if has_owner {
        Some("owned_window")
    } else if class_name != GAME_WINDOW_CLASS {
        Some("class_not_sdl_app")
    } else {
        None
    }
}

fn window_inspection_status(
    process_creation_time_matches: Option<bool>,
    eligible_window_count: usize,
) -> &'static str {
    match process_creation_time_matches {
        Some(false) => "process_identity_mismatch",
        None => "process_identity_unverified",
        Some(true) if eligible_window_count == 0 => "pending_game_window",
        Some(true) if eligible_window_count > 1 => "ambiguous_game_windows",
        Some(true) => "ready",
    }
}

#[cfg(windows)]
fn inspect_broker_window(target: BrokerWindowTarget) -> Result<ProcessWindowInspection, String> {
    let inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    if !inspection.management_eligible {
        return Err(format!(
            "PID {} broker window is not safe to control: {}",
            target.pid, inspection.status
        ));
    }
    if inspection.selected_hwnd_value != Some(target.hwnd_value) {
        return Err(format!(
            "PID {} HWND generation {} is stale",
            target.pid, target.hwnd_generation
        ));
    }
    Ok(inspection)
}

#[cfg(windows)]
fn control_observation(
    action: &'static str,
    target: BrokerWindowTarget,
    inspection: &ProcessWindowInspection,
) -> Result<WindowControlObservation, String> {
    let candidate = selected_candidate(inspection)
        .ok_or_else(|| format!("PID {} selected window disappeared", target.pid))?;
    Ok(WindowControlObservation {
        action,
        pid: target.pid,
        hwnd: candidate.hwnd.clone(),
        hwnd_generation: target.hwnd_generation,
        is_minimized: candidate.is_minimized,
        dpi: candidate.dpi,
    })
}

#[cfg(windows)]
pub fn minimize_broker_window(
    target: BrokerWindowTarget,
) -> Result<WindowControlObservation, String> {
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_MINIMIZE};
    inspect_broker_window(target)?;
    unsafe { ShowWindowAsync(target.hwnd_value as HWND, SW_MINIMIZE) };
    let mut inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    for _ in 0..20 {
        if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized)
            == Some(true)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
        inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    }
    if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized) != Some(true) {
        return Err(format!(
            "PID {} did not enter the minimized state",
            target.pid
        ));
    }
    control_observation("minimize", target, &inspection)
}

#[cfg(windows)]
pub fn restore_broker_window(
    target: BrokerWindowTarget,
) -> Result<WindowControlObservation, String> {
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_RESTORE};
    inspect_broker_window(target)?;
    unsafe { ShowWindowAsync(target.hwnd_value as HWND, SW_RESTORE) };
    let mut inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    for _ in 0..20 {
        if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized)
            == Some(false)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
        inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    }
    if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized) != Some(false) {
        return Err(format!(
            "PID {} did not leave the minimized state",
            target.pid
        ));
    }
    control_observation("restore", target, &inspection)
}

#[cfg(windows)]
pub fn raise_broker_window(target: BrokerWindowTarget) -> Result<WindowControlObservation, String> {
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindowAsync, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };
    inspect_broker_window(target)?;
    // SW_RESTORE activates the target. Keep the launcher (or whichever app the
    // user is operating) in the foreground while revealing the whole group.
    unsafe { ShowWindowAsync(target.hwnd_value as HWND, SW_SHOWNOACTIVATE) };
    let mut inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    for _ in 0..20 {
        if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized)
            == Some(false)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
        inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    }
    if selected_candidate(&inspection).and_then(|candidate| candidate.is_minimized) != Some(false) {
        return Err(format!(
            "PID {} could not be shown without activation",
            target.pid
        ));
    }
    // A plain HWND_TOP request can remain below the current foreground window
    // because Windows protects foreground activation. Promote into the topmost
    // band and immediately demote again; NOACTIVATE keeps keyboard focus where
    // it was while leaving the game above other ordinary windows.
    let flags = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW;
    if unsafe { SetWindowPos(target.hwnd_value as HWND, HWND_TOPMOST, 0, 0, 0, 0, flags) } == 0 {
        return Err(format!(
            "SetWindowPos(group_raise) failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    if unsafe { SetWindowPos(target.hwnd_value as HWND, HWND_NOTOPMOST, 0, 0, 0, 0, flags) } == 0 {
        return Err(format!(
            "SetWindowPos(group_raise demote) failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    let inspection = inspect_process_windows(target.pid, Some(target.process_creation_time));
    control_observation("group_raise", target, &inspection)
}

#[cfg(windows)]
pub fn close_broker_window(target: BrokerWindowTarget) -> Result<WindowControlObservation, String> {
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

    let inspection = inspect_broker_window(target)?;
    let observation = control_observation("close_secondary", target, &inspection)?;
    if unsafe { PostMessageW(target.hwnd_value as HWND, WM_CLOSE, 0, 0) } == 0 {
        return Err(format!(
            "PostMessageW(WM_CLOSE) failed with Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    Ok(observation)
}

#[cfg(not(windows))]
pub fn minimize_broker_window(
    target: BrokerWindowTarget,
) -> Result<WindowControlObservation, String> {
    Err(format!(
        "window minimize is available only on Windows (pid {})",
        target.pid
    ))
}

#[cfg(not(windows))]
pub fn restore_broker_window(
    target: BrokerWindowTarget,
) -> Result<WindowControlObservation, String> {
    Err(format!(
        "window restore is available only on Windows (pid {})",
        target.pid
    ))
}

#[cfg(not(windows))]
pub fn raise_broker_window(target: BrokerWindowTarget) -> Result<WindowControlObservation, String> {
    Err(format!(
        "window raise is available only on Windows (pid {})",
        target.pid
    ))
}

#[cfg(not(windows))]
pub fn close_broker_window(target: BrokerWindowTarget) -> Result<WindowControlObservation, String> {
    Err(format!(
        "window close is available only on Windows (pid {})",
        target.pid
    ))
}

#[cfg(windows)]
fn hwnd_hex(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
    format!("0x{:X}", hwnd as usize)
}

#[cfg(windows)]
fn signed_rect(rect: windows_sys::Win32::Foundation::RECT) -> SignedRect {
    SignedRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

#[cfg(windows)]
pub fn query_process_creation_time(pid: u32) -> Result<u64, String> {
    use std::mem::zeroed;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "OpenProcess({pid}) failed with Windows error {error}"
        ));
    }

    let mut creation: FILETIME = unsafe { zeroed() };
    let mut exit: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    let ok = unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    let error = if ok == 0 {
        Some(unsafe { GetLastError() })
    } else {
        None
    };
    unsafe {
        CloseHandle(process);
    }

    if let Some(error) = error {
        return Err(format!(
            "GetProcessTimes({pid}) failed with Windows error {error}"
        ));
    }

    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

#[cfg(not(windows))]
pub fn query_process_creation_time(pid: u32) -> Result<u64, String> {
    Err(format!(
        "process creation time diagnostics are available only on Windows (pid {pid})"
    ))
}

#[cfg(windows)]
pub fn inspect_process_windows(
    pid: u32,
    expected_process_creation_time_filetime_100ns: Option<u64>,
) -> ProcessWindowInspection {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{GetLastError, BOOL, HWND, LPARAM, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindow, GetWindowLongPtrW, GetWindowPlacement,
        GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, GW_OWNER,
        SW_MINIMIZE, SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE, WINDOWPLACEMENT,
    };

    struct CallbackState {
        pid: u32,
        candidates: Vec<WindowCandidateDiagnostic>,
        warnings: Vec<String>,
    }

    fn hwnd_text(hwnd: HWND, class_name: bool) -> String {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW};

        let mut buffer = vec![0u16; 512];
        let length = unsafe {
            if class_name {
                GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32)
            } else {
                GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32)
            }
        };
        if length <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buffer[..length as usize])
        }
    }

    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: EnumWindows invokes this synchronously while `data` points to
        // the CallbackState owned by inspect_process_windows.
        let state = unsafe { &mut *(data as *mut CallbackState) };
        let mut window_pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut window_pid);
        }
        if window_pid != state.pid {
            return 1;
        }

        let class_name = hwnd_text(hwnd, true);
        let title = hwnd_text(hwnd, false);
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
        let has_owner = !owner.is_null();
        let exclusion_reason = game_window_exclusion_reason(&class_name, has_owner);
        let eligible_game_window = exclusion_reason.is_none();
        let is_visible = unsafe { IsWindowVisible(hwnd) != 0 };

        // Keep the diagnostic bounded to the two user-facing windows and any
        // eligible SDL candidate. IME/GDI helper windows are intentionally omitted.
        if !eligible_game_window && !is_visible && !class_name.starts_with("WindowsForms10.Window")
        {
            return 1;
        }

        let mut placement: WINDOWPLACEMENT = unsafe { zeroed() };
        placement.length = size_of::<WINDOWPLACEMENT>() as u32;
        let placement_ok = unsafe { GetWindowPlacement(hwnd, &mut placement) != 0 };
        if !placement_ok {
            let error = unsafe { GetLastError() };
            state.warnings.push(format!(
                "GetWindowPlacement({}) failed with Windows error {error}",
                hwnd_hex(hwnd)
            ));
        }

        let mut window_rect: RECT = unsafe { zeroed() };
        let window_rect = if unsafe { GetWindowRect(hwnd, &mut window_rect) != 0 } {
            Some(signed_rect(window_rect))
        } else {
            None
        };

        let mut client_rect: RECT = unsafe { zeroed() };
        let client_rect_screen = if unsafe { GetClientRect(hwnd, &mut client_rect) != 0 }
            && client_rect.right > client_rect.left
            && client_rect.bottom > client_rect.top
        {
            let mut origin = POINT { x: 0, y: 0 };
            if unsafe { ClientToScreen(hwnd, &mut origin) != 0 } {
                let width = client_rect.right - client_rect.left;
                let height = client_rect.bottom - client_rect.top;
                match (origin.x.checked_add(width), origin.y.checked_add(height)) {
                    (Some(right), Some(bottom)) => Some(SignedRect {
                        left: origin.x,
                        top: origin.y,
                        right,
                        bottom,
                    }),
                    _ => {
                        state
                            .warnings
                            .push(format!("client rect overflow for {}", hwnd_hex(hwnd)));
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let show_cmd = placement_ok.then_some(placement.showCmd);
        let is_minimized = show_cmd.map(|value| {
            value == SW_SHOWMINIMIZED as u32
                || value == SW_MINIMIZE as u32
                || value == SW_SHOWMINNOACTIVE as u32
        });

        state.candidates.push(WindowCandidateDiagnostic {
            hwnd: hwnd_hex(hwnd),
            class_name,
            title,
            is_visible,
            has_owner,
            eligible_game_window,
            exclusion_reason,
            show_cmd,
            is_minimized,
            dpi: (dpi != 0).then_some(dpi),
            style: format!("0x{:08X}", unsafe {
                GetWindowLongPtrW(hwnd, GWL_STYLE) as u32
            }),
            ex_style: format!("0x{:08X}", unsafe {
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32
            }),
            window_rect,
            normal_rect: placement_ok.then_some(signed_rect(placement.rcNormalPosition)),
            client_rect_screen,
        });
        1
    }

    let observed_creation = query_process_creation_time(pid);
    let (observed_process_creation_time_filetime_100ns, creation_error) = match observed_creation {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let process_creation_time_matches = match (
        expected_process_creation_time_filetime_100ns,
        observed_process_creation_time_filetime_100ns,
    ) {
        (Some(expected), Some(observed)) => Some(expected == observed),
        _ => None,
    };

    let mut state = CallbackState {
        pid,
        candidates: Vec::new(),
        warnings: creation_error.into_iter().collect(),
    };
    let enum_ok =
        unsafe { EnumWindows(Some(callback), &mut state as *mut CallbackState as LPARAM) };
    if enum_ok == 0 {
        let error = unsafe { GetLastError() };
        state
            .warnings
            .push(format!("EnumWindows failed with Windows error {error}"));
    }

    state.candidates.sort_by(|left, right| {
        right
            .eligible_game_window
            .cmp(&left.eligible_game_window)
            .then_with(|| left.class_name.cmp(&right.class_name))
            .then_with(|| left.hwnd.cmp(&right.hwnd))
    });
    let eligible: Vec<&WindowCandidateDiagnostic> = state
        .candidates
        .iter()
        .filter(|candidate| candidate.eligible_game_window)
        .collect();
    let selected_hwnd = (eligible.len() == 1).then(|| eligible[0].hwnd.clone());
    let selected_hwnd_value = (eligible.len() == 1).then(|| {
        usize::from_str_radix(eligible[0].hwnd.trim_start_matches("0x"), 16)
            .expect("HWND diagnostic is generated from usize")
    });

    let status = window_inspection_status(process_creation_time_matches, eligible.len());

    ProcessWindowInspection {
        pid,
        expected_process_creation_time_filetime_100ns:
            expected_process_creation_time_filetime_100ns.map(|value| value.to_string()),
        observed_process_creation_time_filetime_100ns:
            observed_process_creation_time_filetime_100ns.map(|value| value.to_string()),
        process_creation_time_matches,
        selected_hwnd,
        management_eligible: status == "ready",
        status,
        candidates: state.candidates,
        warnings: state.warnings,
        selected_hwnd_value,
    }
}

#[cfg(windows)]
pub fn capture_game_window(
    pid: u32,
    process_creation_time_filetime_100ns: u64,
) -> Result<SavedWindowPlacement, String> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};

    let inspection = inspect_process_windows(pid, Some(process_creation_time_filetime_100ns));
    if !inspection.management_eligible {
        return Err(format!(
            "game window is not safe to control for PID {pid}: {}",
            inspection.status
        ));
    }
    let hwnd_value = inspection
        .selected_hwnd_value
        .ok_or_else(|| format!("selected HWND is missing for PID {pid}"))?;
    let hwnd = hwnd_value as HWND;
    let mut placement: WINDOWPLACEMENT = unsafe { zeroed() };
    placement.length = size_of::<WINDOWPLACEMENT>() as u32;
    if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "GetWindowPlacement({}) failed with Windows error {error}",
            hwnd_hex(hwnd)
        ));
    }

    Ok(SavedWindowPlacement {
        pid,
        process_creation_time_filetime_100ns,
        hwnd_value,
        flags: placement.flags,
        show_cmd: placement.showCmd,
        min_position_x: placement.ptMinPosition.x,
        min_position_y: placement.ptMinPosition.y,
        max_position_x: placement.ptMaxPosition.x,
        max_position_y: placement.ptMaxPosition.y,
        normal_rect: signed_rect(placement.rcNormalPosition),
    })
}

#[cfg(not(windows))]
pub fn capture_game_window(
    pid: u32,
    _process_creation_time_filetime_100ns: u64,
) -> Result<SavedWindowPlacement, String> {
    Err(format!(
        "window capture is available only on Windows (pid {pid})"
    ))
}

#[cfg(windows)]
pub fn apply_test_outer_rect(
    snapshot: &SavedWindowPlacement,
    target: SignedRect,
) -> Result<WindowActionResult, String> {
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindowAsync, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
        SWP_NOZORDER, SW_RESTORE,
    };

    let width = i32::try_from(target.width())
        .map_err(|_| "requested window width is outside i32".to_string())?;
    let height = i32::try_from(target.height())
        .map_err(|_| "requested window height is outside i32".to_string())?;
    if width <= 0 || height <= 0 {
        return Err(format!(
            "requested outer rect must be positive: {}x{}",
            target.width(),
            target.height()
        ));
    }

    revalidate_saved_window(snapshot)?;
    let hwnd = snapshot.hwnd_value as HWND;
    // ShowWindowAsync returns the previous visibility state, not a reliable
    // success flag. The post-action inspection below is the authority.
    unsafe {
        ShowWindowAsync(hwnd, SW_RESTORE);
    }
    let flags = SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOZORDER;
    if unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            target.left,
            target.top,
            width,
            height,
            flags,
        )
    } == 0
    {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "SetWindowPos({}) failed with Windows error {error}",
            hwnd_hex(hwnd)
        ));
    }

    let mut inspection = inspect_process_windows(
        snapshot.pid,
        Some(snapshot.process_creation_time_filetime_100ns),
    );
    for _ in 0..20 {
        if selected_candidate(&inspection).and_then(|candidate| candidate.window_rect)
            == Some(target)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
        inspection = inspect_process_windows(
            snapshot.pid,
            Some(snapshot.process_creation_time_filetime_100ns),
        );
    }
    window_action_result("move_test", &inspection, Some(target))
}

#[cfg(windows)]
pub fn apply_test_position_only(
    snapshot: &SavedWindowPlacement,
    left: i32,
    top: i32,
) -> Result<WindowActionResult, String> {
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GetLastError, HWND};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindowAsync, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOOWNERZORDER,
        SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE,
    };

    let width = i32::try_from(snapshot.normal_rect.width())
        .map_err(|_| "saved window width is outside i32".to_string())?;
    let height = i32::try_from(snapshot.normal_rect.height())
        .map_err(|_| "saved window height is outside i32".to_string())?;
    let right = left
        .checked_add(width)
        .ok_or_else(|| "requested right coordinate is outside i32".to_string())?;
    let bottom = top
        .checked_add(height)
        .ok_or_else(|| "requested bottom coordinate is outside i32".to_string())?;
    let expected = SignedRect {
        left,
        top,
        right,
        bottom,
    };

    revalidate_saved_window(snapshot)?;
    let hwnd = snapshot.hwnd_value as HWND;
    unsafe {
        ShowWindowAsync(hwnd, SW_RESTORE);
    }
    let flags = SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSIZE | SWP_NOZORDER;
    if unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), left, top, 0, 0, flags) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "SetWindowPos(position-only, {}) failed with Windows error {error}",
            hwnd_hex(hwnd)
        ));
    }

    let mut inspection = inspect_process_windows(
        snapshot.pid,
        Some(snapshot.process_creation_time_filetime_100ns),
    );
    for _ in 0..20 {
        if selected_candidate(&inspection)
            .and_then(|candidate| candidate.window_rect)
            .is_some_and(|actual| actual.left == left && actual.top == top)
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
        inspection = inspect_process_windows(
            snapshot.pid,
            Some(snapshot.process_creation_time_filetime_100ns),
        );
    }
    window_action_result("position_only_test", &inspection, Some(expected))
}

#[cfg(not(windows))]
pub fn apply_test_position_only(
    snapshot: &SavedWindowPlacement,
    _left: i32,
    _top: i32,
) -> Result<WindowActionResult, String> {
    Err(format!(
        "window positioning is available only on Windows (pid {})",
        snapshot.pid
    ))
}

#[cfg(not(windows))]
pub fn apply_test_outer_rect(
    snapshot: &SavedWindowPlacement,
    _target: SignedRect,
) -> Result<WindowActionResult, String> {
    Err(format!(
        "window movement is available only on Windows (pid {})",
        snapshot.pid
    ))
}

#[cfg(windows)]
pub fn restore_saved_window(snapshot: &SavedWindowPlacement) -> Result<WindowActionResult, String> {
    use std::mem::{size_of, zeroed};
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GetLastError, HWND, POINT, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPlacement, WINDOWPLACEMENT};

    revalidate_saved_window(snapshot)?;
    let hwnd = snapshot.hwnd_value as HWND;
    let mut placement: WINDOWPLACEMENT = unsafe { zeroed() };
    placement.length = size_of::<WINDOWPLACEMENT>() as u32;
    placement.flags = snapshot.flags;
    placement.showCmd = snapshot.show_cmd;
    placement.ptMinPosition = POINT {
        x: snapshot.min_position_x,
        y: snapshot.min_position_y,
    };
    placement.ptMaxPosition = POINT {
        x: snapshot.max_position_x,
        y: snapshot.max_position_y,
    };
    placement.rcNormalPosition = RECT {
        left: snapshot.normal_rect.left,
        top: snapshot.normal_rect.top,
        right: snapshot.normal_rect.right,
        bottom: snapshot.normal_rect.bottom,
    };
    if unsafe { SetWindowPlacement(hwnd, &placement) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "SetWindowPlacement({}) failed with Windows error {error}",
            hwnd_hex(hwnd)
        ));
    }

    thread::sleep(Duration::from_millis(100));
    let inspection = inspect_process_windows(
        snapshot.pid,
        Some(snapshot.process_creation_time_filetime_100ns),
    );
    window_action_result("restore_test", &inspection, None)
}

#[cfg(not(windows))]
pub fn restore_saved_window(snapshot: &SavedWindowPlacement) -> Result<WindowActionResult, String> {
    Err(format!(
        "window restoration is available only on Windows (pid {})",
        snapshot.pid
    ))
}

#[cfg(windows)]
fn revalidate_saved_window(snapshot: &SavedWindowPlacement) -> Result<(), String> {
    let inspection = inspect_process_windows(
        snapshot.pid,
        Some(snapshot.process_creation_time_filetime_100ns),
    );
    if !inspection.management_eligible {
        return Err(format!(
            "saved window identity is no longer valid for PID {}: {}",
            snapshot.pid, inspection.status
        ));
    }
    if inspection.selected_hwnd_value != Some(snapshot.hwnd_value) {
        return Err(format!(
            "game HWND changed for PID {} (saved {}, current {})",
            snapshot.pid,
            format!("0x{:X}", snapshot.hwnd_value),
            inspection.selected_hwnd.as_deref().unwrap_or("none")
        ));
    }
    Ok(())
}

fn selected_candidate(inspection: &ProcessWindowInspection) -> Option<&WindowCandidateDiagnostic> {
    let selected = inspection.selected_hwnd.as_deref()?;
    inspection
        .candidates
        .iter()
        .find(|candidate| candidate.hwnd == selected)
}

fn window_action_result(
    action: &'static str,
    inspection: &ProcessWindowInspection,
    requested_outer_rect: Option<SignedRect>,
) -> Result<WindowActionResult, String> {
    if !inspection.management_eligible {
        return Err(format!(
            "game window became unsafe after {action} for PID {}: {}",
            inspection.pid, inspection.status
        ));
    }
    let candidate = selected_candidate(inspection)
        .ok_or_else(|| format!("selected game window disappeared after {action}"))?;
    Ok(WindowActionResult {
        action,
        pid: inspection.pid,
        hwnd: candidate.hwnd.clone(),
        requested_outer_rect,
        actual_window_rect: candidate.window_rect,
        actual_normal_rect: candidate.normal_rect,
        actual_client_rect_screen: candidate.client_rect_screen,
        requested_outer_rect_matches: requested_outer_rect
            .map(|requested| candidate.window_rect == Some(requested)),
        requested_position_matches: requested_outer_rect.map(|requested| {
            candidate
                .window_rect
                .is_some_and(|actual| actual.left == requested.left && actual.top == requested.top)
        }),
        show_cmd: candidate.show_cmd,
        is_minimized: candidate.is_minimized,
        dpi: candidate.dpi,
    })
}

#[cfg(not(windows))]
pub fn inspect_process_windows(
    pid: u32,
    expected_process_creation_time_filetime_100ns: Option<u64>,
) -> ProcessWindowInspection {
    ProcessWindowInspection {
        pid,
        expected_process_creation_time_filetime_100ns:
            expected_process_creation_time_filetime_100ns.map(|value| value.to_string()),
        observed_process_creation_time_filetime_100ns: None,
        process_creation_time_matches: None,
        selected_hwnd: None,
        management_eligible: false,
        status: "unsupported_platform",
        candidates: Vec::new(),
        warnings: vec!["window diagnostics are available only on Windows".to_string()],
        selected_hwnd_value: None,
    }
}

#[cfg(windows)]
pub fn enumerate_monitors() -> Result<MonitorEnumeration, String> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{GetLastError, BOOL, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    struct CallbackState {
        monitors: Vec<MonitorDescriptor>,
        warnings: Vec<String>,
    }

    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` points to CallbackState for the synchronous duration of
        // EnumDisplayMonitors below.
        let state = unsafe { &mut *(data as *mut CallbackState) };
        let mut info: MONITORINFOEXW = unsafe { zeroed() };
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;

        // MONITORINFOEXW starts with MONITORINFO, as required by GetMonitorInfoW.
        let ok = unsafe {
            GetMonitorInfoW(
                monitor,
                &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
            )
        };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            state
                .warnings
                .push(format!("GetMonitorInfoW failed with Windows error {error}"));
            return 1;
        }

        let name_len = info
            .szDevice
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(info.szDevice.len());
        let device_name = String::from_utf16_lossy(&info.szDevice[..name_len]);
        let monitor_rect = info.monitorInfo.rcMonitor;
        let work_area = info.monitorInfo.rcWork;

        state.monitors.push(MonitorDescriptor {
            device_name,
            is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            monitor_rect: SignedRect {
                left: monitor_rect.left,
                top: monitor_rect.top,
                right: monitor_rect.right,
                bottom: monitor_rect.bottom,
            },
            work_area: SignedRect {
                left: work_area.left,
                top: work_area.top,
                right: work_area.right,
                bottom: work_area.bottom,
            },
        });
        1
    }

    let mut state = CallbackState {
        monitors: Vec::new(),
        warnings: Vec::new(),
    };
    let ok = unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(callback),
            &mut state as *mut CallbackState as LPARAM,
        )
    };
    if ok == 0 {
        let error = unsafe { GetLastError() };
        return Err(format!(
            "EnumDisplayMonitors failed with Windows error {error}"
        ));
    }

    state
        .monitors
        .sort_by(|left, right| left.device_name.cmp(&right.device_name));

    Ok(MonitorEnumeration {
        monitors: state.monitors,
        warnings: state.warnings,
    })
}

#[cfg(not(windows))]
pub fn enumerate_monitors() -> Result<MonitorEnumeration, String> {
    Err("Stage 0 monitor diagnostics are available only on Windows".to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn selects_sdl_app_by_class_and_owner_not_mutable_title() {
        assert_eq!(game_window_exclusion_reason("SDL_app", false), None);
        assert_eq!(
            game_window_exclusion_reason("SDL_app", true),
            Some("owned_window")
        );
        assert_eq!(
            game_window_exclusion_reason("WindowsForms10.Window.208.app", false),
            Some("class_not_sdl_app")
        );
    }

    #[test]
    fn refuses_unverified_mismatched_or_ambiguous_window_identity() {
        assert_eq!(
            window_inspection_status(None, 1),
            "process_identity_unverified"
        );
        assert_eq!(
            window_inspection_status(Some(false), 1),
            "process_identity_mismatch"
        );
        assert_eq!(
            window_inspection_status(Some(true), 0),
            "pending_game_window"
        );
        assert_eq!(window_inspection_status(Some(true), 1), "ready");
        assert_eq!(
            window_inspection_status(Some(true), 2),
            "ambiguous_game_windows"
        );
    }

    #[test]
    fn reads_and_revalidates_current_process_creation_time() {
        let pid = std::process::id();
        let creation = query_process_creation_time(pid).expect("current process must be queryable");
        let inspection = inspect_process_windows(pid, Some(creation));

        assert_eq!(inspection.process_creation_time_matches, Some(true));
        assert_ne!(inspection.status, "process_identity_mismatch");
    }

    #[test]
    fn enumerates_current_windows_monitors_read_only() {
        let enumeration = enumerate_monitors().expect("monitor enumeration must succeed");
        assert!(!enumeration.monitors.is_empty());

        for monitor in &enumeration.monitors {
            assert!(monitor.monitor_rect.width() > 0);
            assert!(monitor.monitor_rect.height() > 0);
            assert!(monitor.work_area.width() > 0);
            assert!(monitor.work_area.height() > 0);
            assert!(monitor.work_area.left >= monitor.monitor_rect.left);
            assert!(monitor.work_area.top >= monitor.monitor_rect.top);
            assert!(monitor.work_area.right <= monitor.monitor_rect.right);
            assert!(monitor.work_area.bottom <= monitor.monitor_rect.bottom);
        }

        eprintln!(
            "STAGE0_MONITOR_DIAGNOSTICS={} ",
            serde_json::to_string_pretty(&enumeration).unwrap()
        );
    }
}
