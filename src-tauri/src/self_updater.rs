//! 런처 자기 업데이트.
//!
//! 흐름:
//!   1) ggoce-deploy의 launcher/manifest.json fetch
//!   2) 현재 exe 버전과 비교
//!   3) 새 exe를 %TEMP%\GGOLauncher.exe.new로 다운로드 + SHA256 검증
//!   4) updater.bat 생성 + 실행 후 런처 종료 (batch가 swap 수행)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 런처 manifest 후보 URL (순서대로 시도). raw 차단 환경 폴백용 jsDelivr 미러 포함.
const MANIFEST_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/launcher/manifest.json",
    "https://cdn.jsdelivr.net/gh/gu2tarman/ggoce-deploy@main/launcher/manifest.json",
];

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherManifest {
    pub version: String,
    #[serde(default)]
    pub released: String,
    #[serde(default)]
    pub notes: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfUpdateCheck {
    pub current_version: String,
    pub remote_version: String,
    pub update_available: bool,
    /// remote가 더 최신이면 manifest 보관 (apply 시 재사용).
    pub manifest: Option<LauncherManifest>,
}

pub async fn check() -> Result<SelfUpdateCheck, String> {
    let manifest = fetch_manifest().await?;
    let update_available = is_newer(&manifest.version, CURRENT_VERSION);
    Ok(SelfUpdateCheck {
        current_version: CURRENT_VERSION.to_string(),
        remote_version: manifest.version.clone(),
        update_available,
        manifest: if update_available {
            Some(manifest)
        } else {
            None
        },
    })
}

/// 단순 semver 비교: "0.1.0" vs "0.2.0" 등.
/// 잘못된 형식이면 문자열 비교로 폴백.
fn is_newer(remote: &str, current: &str) -> bool {
    let parse =
        |s: &str| -> Option<Vec<u32>> { s.split('.').map(|x| x.parse::<u32>().ok()).collect() };
    match (parse(remote), parse(current)) {
        (Some(r), Some(c)) => r > c,
        _ => remote > current,
    }
}

pub async fn download_and_apply<F>(
    manifest: &LauncherManifest,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    use futures_util::StreamExt;

    // manifest URL HTTPS 강제 (downgrade 방어)
    if !manifest.url.to_ascii_lowercase().starts_with("https://") {
        return Err("런처 manifest url은 https://여야 합니다".into());
    }

    // 1) 임시 위치로 다운로드
    let temp_dir = std::env::temp_dir();
    let new_exe = temp_dir.join("GGOLauncher.exe.new");
    if new_exe.exists() {
        std::fs::remove_file(&new_exe).ok();
    }

    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(&manifest.url)
        .send()
        .await
        .map_err(|e| format!("런처 다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} ({})", resp.status(), manifest.url));
    }

    let total = manifest.size;
    let mut done: u64 = 0;
    on_progress(0, total);

    let mut file = tokio::fs::File::create(&new_exe)
        .await
        .map_err(|e| format!("임시 파일 생성 실패: {e}"))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("스트림 읽기 실패: {e}"))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &bytes)
            .await
            .map_err(|e| format!("쓰기 실패: {e}"))?;
        done = done.saturating_add(bytes.len() as u64);
        on_progress(done, total);
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| format!("flush 실패: {e}"))?;
    drop(file);

    // 2) SHA256 검증
    let actual = sha256_file(&new_exe)?;
    if !actual.eq_ignore_ascii_case(&manifest.sha256) {
        let _ = std::fs::remove_file(&new_exe);
        return Err(format!(
            "해시 불일치 (expected={}, got={actual})",
            manifest.sha256
        ));
    }

    // 3) updater.bat 생성 + 실행, 런처 자기 종료는 호출자(lib.rs)가 처리
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let bat = temp_dir.join("ggolauncher_updater.bat");
    write_updater_bat(&bat, &new_exe, &current_exe)?;

    spawn_bat(&bat)?;
    Ok(())
}

fn write_updater_bat(
    bat: &PathBuf,
    new_exe: &PathBuf,
    current_exe: &PathBuf,
) -> Result<(), String> {
    // CRLF + ASCII only. EnableDelayedExpansion 필수(!RETRY! 사용).
    // 전략:
    //   1) 기존 exe를 .old로 rename (삭제 X) — 실패 시 retry, 모두 실패면 그대로 종료.
    //   2) 새 exe를 원본 자리로 move.
    //   3) move 성공 시 .old 삭제(실패해도 무시), 새 런처 실행.
    //   4) move 실패 시 .old를 원위치로 복구하고 종료.
    let old_exe = format!("{}.old", current_exe.display());
    let script = format!(
        "@echo off\r\n\
         setlocal EnableExtensions EnableDelayedExpansion\r\n\
         rem GGO Launcher 자기 업데이트 헬퍼\r\n\
         rem 1. 런처 종료될 시간 확보\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         rem 2. 기존 exe → .old 로 rename (사용 중이면 retry)\r\n\
         if exist \"{old}\" del /f /q \"{old}\" >nul 2>&1\r\n\
         set RETRY=0\r\n\
         :retry_rename\r\n\
         if exist \"{cur}\" (\r\n\
             ren \"{cur}\" \"{cur_name}.old\" >nul 2>&1\r\n\
             if exist \"{cur}\" (\r\n\
                 set /a RETRY+=1\r\n\
                 if !RETRY! lss 10 (\r\n\
                     timeout /t 1 /nobreak >nul\r\n\
                     goto retry_rename\r\n\
                 ) else (\r\n\
                     echo [updater] 기존 exe rename 실패 - 수동 종료 후 재시도 필요\r\n\
                     pause\r\n\
                     exit /b 1\r\n\
                 )\r\n\
             )\r\n\
         )\r\n\
         rem 3. 새 exe move\r\n\
         move /y \"{new}\" \"{cur}\" >nul\r\n\
         if errorlevel 1 (\r\n\
             echo [updater] 파일 교체 실패 - 이전 버전 복구\r\n\
             if exist \"{old}\" ren \"{old}\" \"{cur_name}\" >nul 2>&1\r\n\
             pause\r\n\
             exit /b 1\r\n\
         )\r\n\
         rem 4. 이전 버전 삭제 (실패해도 무시)\r\n\
         if exist \"{old}\" del /f /q \"{old}\" >nul 2>&1\r\n\
         rem 5. 새 런처 실행\r\n\
         start \"\" \"{cur}\"\r\n\
         rem 6. 자기 삭제\r\n\
         (goto) 2>nul & del \"%~f0\"\r\n",
        cur = current_exe.display(),
        cur_name = current_exe
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("GGOLauncher.exe"),
        new = new_exe.display(),
        old = old_exe,
    );
    std::fs::write(bat, script).map_err(|e| format!("bat 생성 실패: {e}"))
}

#[cfg(windows)]
fn spawn_bat(bat: &PathBuf) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    std::process::Command::new("cmd")
        .args(["/C", "call"])
        .arg(bat.as_os_str())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("updater.bat spawn 실패: {e}"))
}

#[cfg(not(windows))]
fn spawn_bat(_bat: &PathBuf) -> Result<(), String> {
    Err("self_updater는 Windows 전용".to_string())
}

fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

async fn fetch_manifest() -> Result<LauncherManifest, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    // 1순위(raw) 실패 시 2순위(jsDelivr)로 폴백. 어느 소스·무슨 에러였는지 누적.
    let mut errors = String::new();
    for url in MANIFEST_URLS {
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => {
                return resp
                    .json::<LauncherManifest>()
                    .await
                    .map_err(|e| format!("manifest JSON 파싱 실패: {e}"));
            }
            Ok(resp) => {
                if !errors.is_empty() {
                    errors.push_str(" | ");
                }
                errors.push_str(&format!("[{url}] HTTP {}", resp.status()));
            }
            Err(e) => {
                if !errors.is_empty() {
                    errors.push_str(" | ");
                }
                errors.push_str(&format!("[{url}] {e}"));
            }
        }
    }
    Err(format!("런처 manifest 가져오기 실패 (모든 소스): {errors}"))
}
