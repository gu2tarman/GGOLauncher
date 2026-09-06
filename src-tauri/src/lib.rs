mod crypto;
mod cuo_profiles;
mod launcher;
mod multiclient;
mod notice;
mod paths;
mod plugins;
mod profile;
mod self_updater;
mod settings;
mod sidebar;
mod updater;

use notice::NoticeBoard;
use paths::{FolderKind, PathInfo};
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
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(cmd)
            .arg(&url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL이 비어있습니다".into());
    }
    if trimmed != url {
        return Err("URL 앞뒤 공백은 허용되지 않습니다".into());
    }
    // 제어 문자/줄바꿈/NUL 거부 — OS 핸들러로 넘기기 전 경계 문자 차단
    if url.chars().any(|c| c.is_control() || c == '\0') {
        return Err("URL에 허용되지 않는 문자가 포함됨".into());
    }
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("URL 형식 오류: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {
            if parsed.host_str().is_none() {
                return Err("http/https URL에는 host가 필요합니다".into());
            }
        }
        "mailto" | "tel" => {
            if parsed.path().is_empty() {
                return Err(format!("{} URL 값이 비어있습니다", parsed.scheme()));
            }
        }
        scheme => {
            return Err(format!("허용되지 않는 URL scheme: {scheme}"));
        }
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
fn detect_folder_kind(path: String) -> FolderKind {
    paths::detect_folder_kind(&path)
}

#[tauri::command]
fn get_launcher_dir() -> Option<String> {
    paths::launcher_dir()
}

#[tauri::command]
fn list_cuo_profiles(cuo_path: String) -> Vec<cuo_profiles::CuoProfileCandidate> {
    cuo_profiles::list(&cuo_path)
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
fn cuo_launch(
    profile_id: String,
    account_id: Option<String>,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<(), String> {
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
    launcher::launch(&profile_id, account.as_ref(), stage0.inner())
}

/// MULTI LOGIN — 프로필 안 모든 계정 순차 spawn. 반환: 실행된 계정 수.
#[tauri::command]
async fn cuo_launch_multi(
    profile_id: String,
    delay_ms: Option<u64>,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<launcher::MultiLaunchResult, String> {
    launcher::launch_multi(&profile_id, delay_ms.unwrap_or(2000), stage0.inner()).await
}

#[tauri::command]
fn multiclient_session_status(
    profile_id: String,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<multiclient::MultiSessionStatus, String> {
    launcher::multi_session_status(&profile_id, stage0.inner())
}

#[tauri::command]
fn multiclient_group_control(
    profile_id: String,
    action: String,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<launcher::GroupControlResult, String> {
    launcher::control_secondary_group(&profile_id, &action, stage0.inner())
}

/// Read-only Stage 0 snapshot. When GGO_MULTICLIENT_STAGE0 is not exactly `1`,
/// the command reports disabled state and performs no monitor enumeration.
#[tauri::command]
fn multiclient_stage0_diagnostics(
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> multiclient::Stage0Diagnostics {
    stage0.diagnostics()
}

/// Explicit diagnostics-only proof that CE reconnects after a transient pipe
/// disconnect while the launcher and game process remain alive.
#[tauri::command]
fn multiclient_stage0_disconnect_broker_test(
    profile_id: String,
    account_id: String,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<(), String> {
    stage0.disconnect_broker_session_for_test(&profile_id, &account_id)
}

/// Explicit Stage 1B proof: move exactly one verified SDL game window to a
/// secondary-monitor 2x2 cell. The original WINDOWPLACEMENT stays in memory.
#[tauri::command]
fn multiclient_stage1b_move_test(
    runtime_session_id: u64,
    slot: String,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<multiclient::SingleWindowTestResult, String> {
    stage0.move_single_window_test(runtime_session_id, &slot)
}

/// Restore the exact WINDOWPLACEMENT captured before Stage 1B moved the window.
#[tauri::command]
fn multiclient_stage1b_restore_test(
    runtime_session_id: u64,
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<multiclient::SingleWindowTestResult, String> {
    stage0.restore_single_window_test(runtime_session_id)
}

/// Position exactly six verified game windows without changing their size:
/// sessions #1-#2 on the primary monitor and #3-#6 on a secondary 2x2 grid.
#[tauri::command]
fn multiclient_stage1b_position_six_test(
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<multiclient::GroupWindowTestResult, String> {
    stage0.position_six_windows_test()
}

/// Restore every in-memory placement captured by explicit window tests.
#[tauri::command]
fn multiclient_stage1b_restore_all_test(
    stage0: tauri::State<'_, multiclient::Stage0State>,
) -> Result<multiclient::GroupWindowTestResult, String> {
    stage0.restore_all_window_tests()
}

// ── 플러그인 ──────────────────────────────────────────────
#[tauri::command]
async fn add_plugin() -> Result<Option<String>, String> {
    plugins::add_plugin().await
}

// ── 공지사항 ──────────────────────────────────────────────
#[tauri::command]
async fn fetch_notice() -> Result<NoticeBoard, String> {
    notice::fetch().await
}

// ── 사이드바 ──────────────────────────────────────────────
#[tauri::command]
async fn fetch_sidebar() -> Result<sidebar::Sidebar, String> {
    sidebar::fetch().await
}

// ── CUO 업데이트 체크 ─────────────────────────────────────
#[tauri::command]
async fn cuo_check_update(cuo_path: String) -> Result<updater::UpdateCheck, String> {
    updater::check_update(&cuo_path).await
}

/// cuo_path가 비었거나 신규 설치인 케이스 — 로컬 검사 스킵하고 manifest만 받아옴.
/// 결과의 `changed`는 manifest의 모든 파일을 Missing으로 표시.
#[tauri::command]
async fn cuo_fetch_manifest_for_install() -> Result<updater::UpdateCheck, String> {
    updater::fetch_manifest_as_install().await
}

/// CUO 업데이트 적용. 진행률은 `cuo_update_progress` 이벤트로 emit
/// (페이로드: { bytesDone: u64, totalBytes: u64 }).
/// allow_original_overwrite: 원본 CUO 폴더 덮어쓰기 허용(사용자 확인 후 true).
#[tauri::command]
async fn cuo_apply_update(
    cuo_path: String,
    check: updater::UpdateCheck,
    allow_original_overwrite: bool,
    window: tauri::Window,
) -> Result<(), String> {
    use tauri::Emitter;
    let win = window.clone();
    updater::apply_update(
        &cuo_path,
        &check,
        allow_original_overwrite,
        move |bytes_done, total| {
            let _ = win.emit(
                "cuo_update_progress",
                serde_json::json!({ "bytesDone": bytes_done, "totalBytes": total }),
            );
        },
    )
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
    let stage0 = multiclient::Stage0State::from_environment();
    tauri::Builder::default()
        .manage(stage0)
        .invoke_handler(tauri::generate_handler![
            open_external,
            launcher_init,
            get_settings,
            save_settings,
            inspect_path,
            detect_client_version,
            detect_ggoce_version,
            detect_folder_kind,
            get_launcher_dir,
            list_cuo_profiles,
            client_select_directory,
            cuo_select_directory,
            cuo_launch,
            cuo_launch_multi,
            multiclient_session_status,
            multiclient_group_control,
            multiclient_stage0_diagnostics,
            multiclient_stage0_disconnect_broker_test,
            multiclient_stage1b_move_test,
            multiclient_stage1b_restore_test,
            multiclient_stage1b_position_six_test,
            multiclient_stage1b_restore_all_test,
            encrypt_password,
            decrypt_password,
            add_plugin,
            fetch_notice,
            fetch_sidebar,
            cuo_check_update,
            cuo_fetch_manifest_for_install,
            cuo_apply_update,
            launcher_check_update,
            launcher_apply_update,
            quit_launcher,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
