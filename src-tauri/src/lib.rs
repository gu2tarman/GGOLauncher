mod launcher;
mod notice;
mod paths;
mod plugins;
mod profile;
mod settings;

use notice::NoticeBoard;
use paths::PathInfo;
use settings::Settings;
use std::process::Command;

// ── 외부 링크 ─────────────────────────────────────────────
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let lower = url.to_lowercase();
    let allowed = lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:");
    if !allowed {
        return Err(format!("Disallowed URL scheme: {url}"));
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        Command::new(cmd)
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
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
fn cuo_launch_multi(profile_id: String, delay_ms: Option<u64>) -> Result<usize, String> {
    launcher::launch_multi(&profile_id, delay_ms.unwrap_or(2000))
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
            plugin_select_file,
            import_plugin_from_zip,
            fetch_notice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
