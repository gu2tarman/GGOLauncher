mod crypto;
mod launcher;
mod notice;
mod paths;
mod plugins;
mod profile;
mod self_updater;
mod settings;
mod updater;

use notice::NoticeBoard;
use paths::PathInfo;
use settings::Settings;

// ── 외부 링크 ─────────────────────────────────────────────
/// 외부 URL을 OS 기본 핸들러로 엽니다.
/// scheme 화이트리스트(http/https/mailto/tel) + control char 거부.
/// Windows는 ShellExecuteW 직접 호출(cmd 미사용 → 메타문자 인젝션 차단).
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    validate_external_url(&url)?;

    #[cfg(target_os = "windows")]
    {
        shell_open_url(&url)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(cmd)
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_external_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("URL이 비어있습니다".into());
    }
    // 제어 문자/줄바꿈/공백 거부 — ShellExecuteW가 해석 가능한 모든 경계 차단
    if url.chars().any(|c| c.is_control() || c == '\n' || c == '\r' || c == '\0') {
        return Err("URL에 허용되지 않는 문자가 포함됨".into());
    }
    let lower = url.to_ascii_lowercase();
    let allowed = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:");
    if !allowed {
        return Err(format!("허용되지 않는 URL scheme: {url}"));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn shell_open_url(url: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = OsStr::new("open").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = OsStr::new(url).encode_wide().chain(once(0)).collect();

    // ShellExecuteW return > 32 = 성공
    let h = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL as i32,
        )
    };
    if (h as isize) <= 32 {
        return Err(format!("ShellExecuteW 실패 (code {})", h as isize));
    }
    Ok(())
}

// ── 설정 / 프로필 ─────────────────────────────────────────
#[tauri::command]
fn launcher_init() -> Result<Settings, String> {
    settings::load()
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    settings::load()
}

#[tauri::command]
fn save_settings(settings: Settings) -> Result<(), String> {
    settings::save(&settings)
}

// ── 경로 / 다이얼로그 ─────────────────────────────────────
#[tauri::command]
fn inspect_path(path: String) -> PathInfo {
    paths::inspect(&path)
}

#[tauri::command]
fn detect_client_version(uo_path: String) -> Option<String> {
    paths::detect_client_version(&uo_path)
}

#[tauri::command]
fn detect_ggoce_version(cuo_path: String) -> Option<String> {
    paths::detect_ggoce_version(&cuo_path)
}

#[tauri::command]
async fn client_select_directory(start_dir: Option<String>) -> Option<String> {
    paths::pick_folder(start_dir, "UO 클라이언트 폴더 선택").await
}

#[tauri::command]
async fn cuo_select_directory(start_dir: Option<String>) -> Option<String> {
    paths::pick_folder(start_dir, "ClassicUO 폴더 선택").await
}

// ── 비밀번호 암복호화 (DPAPI) ─────────────────────────────
/// DPAPI 암호화 — 실패 시 평문 저장 금지(Err 반환).
#[tauri::command]
fn encrypt_password(plain: String) -> Result<String, String> {
    crypto::encrypt(&plain)
}

#[tauri::command]
fn decrypt_password(stored: String) -> String {
    crypto::decrypt_or_passthrough(&stored)
}

// ── PLAY ──────────────────────────────────────────────────
/// 단일 PLAY. account_id가 None이면 무인증 spawn (사용자가 CUO에서 직접 입력).
#[tauri::command]
fn cuo_launch(profile_id: String, account_id: Option<String>) -> Result<(), String> {
    let s = settings::load()?;
    let account = if let Some(aid) = account_id {
        s.profiles
            .iter()
            .find(|p| p.id == profile_id)
            .and_then(|p| p.server.accounts.iter().find(|a| a.id == aid))
            .cloned()
    } else {
        None
    };
    launcher::launch(&profile_id, account.as_ref())
}

/// MULTI LOGIN — 프로필 안 모든 계정 순차 spawn. 반환: 실행된 계정 수.
#[tauri::command]
async fn cuo_launch_multi(profile_id: String, delay_ms: Option<u64>) -> Result<usize, String> {
    launcher::launch_multi(&profile_id, delay_ms.unwrap_or(2000)).await
}

// ── 플러그인 ──────────────────────────────────────────────
#[tauri::command]
async fn plugin_select_file() -> Option<String> {
    plugins::pick_plugin_file().await
}

#[tauri::command]
async fn import_plugin_from_zip() -> Result<Option<String>, String> {
    plugins::import_from_zip().await
}

// ── 공지사항 ──────────────────────────────────────────────
#[tauri::command]
async fn fetch_notice() -> Result<NoticeBoard, String> {
    notice::fetch().await
}

// ── CUO 업데이트 체크 ─────────────────────────────────────
#[tauri::command]
async fn cuo_check_update(cuo_path: String) -> Result<updater::UpdateCheck, String> {
    updater::check_update(&cuo_path).await
}

/// CUO 업데이트 적용. 진행률은 `cuo_update_progress` 이벤트로 emit
/// (페이로드: { bytesDone: u64, totalBytes: u64 }).
#[tauri::command]
async fn cuo_apply_update(
    cuo_path: String,
    check: updater::UpdateCheck,
    window: tauri::Window,
) -> Result<(), String> {
    use tauri::Emitter;
    let win = window.clone();
    updater::apply_update(&cuo_path, &check, move |bytes_done, total| {
        let _ = win.emit(
            "cuo_update_progress",
            serde_json::json!({ "bytesDone": bytes_done, "totalBytes": total }),
        );
    })
    .await
}

// ── 런처 종료 ─────────────────────────────────────────────
#[tauri::command]
fn quit_launcher(app: tauri::AppHandle) {
    app.exit(0);
}

// ── 런처 자기 업데이트 ───────────────────────────────────
#[tauri::command]
async fn launcher_check_update() -> Result<self_updater::SelfUpdateCheck, String> {
    self_updater::check().await
}

/// 다운로드 + updater.bat 실행. 호출 직후 런처 자기 종료해야 batch가 이어받음.
#[tauri::command]
async fn launcher_apply_update(
    manifest: self_updater::LauncherManifest,
    window: tauri::Window,
) -> Result<(), String> {
    use tauri::Emitter;
    let win = window.clone();
    self_updater::download_and_apply(&manifest, move |bytes_done, total| {
        let _ = win.emit(
            "launcher_update_progress",
            serde_json::json!({ "bytesDone": bytes_done, "totalBytes": total }),
        );
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_external,
            launcher_init,
            get_settings,
            save_settings,
            inspect_path,
            detect_client_version,
            detect_ggoce_version,
            client_select_directory,
            cuo_select_directory,
            cuo_launch,
            cuo_launch_multi,
            encrypt_password,
            decrypt_password,
            plugin_select_file,
            import_plugin_from_zip,
            fetch_notice,
            cuo_check_update,
            cuo_apply_update,
            launcher_check_update,
            launcher_apply_update,
            quit_launcher,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
