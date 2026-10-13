//! 트레이 상주: 창을 닫으면 창+WebView2만 파괴하고 프로세스는 남는다.
//! 멀티클라 broker(named pipe)는 백엔드 스레드라 창 없이도 그룹 제어가 유지된다.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

const TRAY_ID: &str = "main";
const WINDOW_LABEL: &str = "main";
const TOOLTIP: &str = "GGO Launcher";

/// 마지막 창 닫힘 => 코드 없는 종료 요청: 트레이에 상주.
/// 명시적 `app.exit(code)`(트레이 종료, 자기 업데이트)는 코드가 있어 그대로 종료.
pub fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

/// 메인 창을 다시 띄운다. 닫혀서 파괴됐으면 설정의 창 정의로 재생성.
pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == WINDOW_LABEL)
        .cloned()
    else {
        return;
    };
    if let Ok(builder) = WebviewWindowBuilder::from_config(app, &config) {
        if let Ok(window) = builder.build() {
            let _ = window.set_focus();
        }
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "런처 종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            // 런처만 종료한다. 실행 중인 게임 클라이언트는 건드리지 않는다.
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// 단일 실행 보장: 첫 런처가 named auto-reset event를 소유하고, 이후 실행은
/// 그 이벤트를 신호(기존 런처가 창을 띄움)한 뒤 바로 종료한다.
pub struct InstanceGuard {
    event: usize, // 대기 스레드가 소유하도록 HANDLE을 정수로 보관
}

// MacroWorker(`GGOWorkerLauncher...`)와 다른 이름 — 둘은 동시에 실행 가능해야 한다.
const SHOW_EVENT_NAME: &str = "Local\\GGOLauncher.ShowWindow";

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

impl InstanceGuard {
    pub fn acquire() -> Option<Self> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateEventW;
        let name = wide(SHOW_EVENT_NAME);
        // SAFETY: NUL 종료 이름, null 보안 속성.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            return None;
        }
        // SAFETY: CreateEventW 직후의 스레드 로컬 에러 조회.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: 위에서 받은 핸들, 공유되지 않음.
            unsafe { CloseHandle(event) };
            return None;
        }
        Some(Self {
            event: event as usize,
        })
    }

    /// 자기 업데이트 직후 실행: 이전 런처가 아직 종료 중일 수 있으니 이벤트가
    /// 풀릴 때까지 `wait` 동안 재시도한다.
    pub fn acquire_waiting(wait: std::time::Duration) -> Option<Self> {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(guard) = Self::acquire() {
                return Some(guard);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    pub fn signal_existing() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};
        let name = wide(SHOW_EVENT_NAME);
        // SAFETY: NUL 종료 이름.
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if !event.is_null() {
            // SAFETY: 위에서 modify 권한으로 연 핸들.
            unsafe {
                SetEvent(event);
                CloseHandle(event);
            }
        }
    }

    /// 프로세스 수명 동안 이벤트를 소유하며, 신호마다 창을 다시 띄운다.
    pub fn listen(self, app: AppHandle) {
        let event = self.event;
        std::thread::spawn(move || {
            use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
            loop {
                // SAFETY: 이벤트 핸들은 프로세스 수명 동안 열려 있다.
                let result = unsafe { WaitForSingleObject(event as _, INFINITE) };
                if result != 0 {
                    return; // WAIT_OBJECT_0만 처리, 그 외에는 리스너 종료.
                }
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || show_main_window(&handle));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::should_prevent_exit;

    #[test]
    fn closing_the_last_window_stays_resident_but_explicit_exit_proceeds() {
        assert!(should_prevent_exit(None));
        assert!(!should_prevent_exit(Some(0)));
        assert!(!should_prevent_exit(Some(1)));
    }
}
