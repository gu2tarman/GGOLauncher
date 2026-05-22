use std::process::Command;

/// 외부 브라우저(또는 OS 기본 핸들러)로 URL 열기.
/// Windows: `cmd /C start "" "<url>"`
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    // 안전 가드: http/https/mailto/tel만 허용
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
        // 미래 대비: macOS는 `open`, Linux는 `xdg-open`
        let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        Command::new(cmd)
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![open_external])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
