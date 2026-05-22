mod notice;
mod paths;
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
async fn client_select_directory(start_dir: Option<String>) -> Option<String> {
    paths::pick_folder(start_dir, "UO 클라이언트 폴더 선택").await
}

#[tauri::command]
async fn cuo_select_directory(start_dir: Option<String>) -> Option<String> {
    paths::pick_folder(start_dir, "ClassicUO 폴더 선택").await
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
            client_select_directory,
            cuo_select_directory,
            fetch_notice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
