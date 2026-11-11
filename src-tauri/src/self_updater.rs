//! 런처 자기 업데이트.
//!
//! 흐름:
//!   1) ggoce-deploy의 launcher/manifest.json fetch
//!   2) 현재 exe 버전과 비교
//!   3) 새 exe를 런처와 같은 폴더의 GGOLauncher.new.exe로 다운로드 + SHA256 검증
//!   4) 실행 중인 exe를 GGOLauncher.old.exe로 rename → 새 exe를 원래 이름으로 rename
//!      (Windows는 실행 중인 exe의 rename을 허용한다)
//!   5) 새 런처를 --after-update로 실행, 기존 런처는 호출자(lib.rs)가 종료
//!   6) 새 런처가 시작하면서 남은 .old.exe / .new.exe를 지운다
//!
//! 임시 폴더·배치 스크립트는 쓰지 않는다(백신 휴리스틱 오탐 원인).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 런처 manifest 후보 URL (순서대로 시도). raw 차단 환경 폴백용 jsDelivr 미러 포함.
const MANIFEST_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/launcher/manifest.json",
    "https://cdn.jsdelivr.net/gh/gu2tarman/ggoce-deploy@main/launcher/manifest.json",
];

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 업데이트 직후 새 런처를 띄울 때 붙이는 인자. 이 인자로 시작하면 이전 런처가
/// 단일 실행 이벤트를 놓을 때까지 기다린다(lib.rs run()).
pub const AFTER_UPDATE_ARG: &str = "--after-update";

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

    // 1) 런처와 같은 폴더로 다운로드
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let new_exe = sibling(&current_exe, "new");
    let old_exe = sibling(&current_exe, "old");
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
        .map_err(|e| format!("업데이트 파일 생성 실패 ({}): {e}", new_exe.display()))?;
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

    // 3) 교체 + 새 런처 실행. 런처 자기 종료는 호출자(lib.rs 백엔드)가 처리
    swap_in(&current_exe, &new_exe, &old_exe)?;
    let mut cmd = std::process::Command::new(&current_exe);
    cmd.arg(AFTER_UPDATE_ARG);
    if let Some(dir) = current_exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("업데이트는 적용됐지만 새 런처 실행 실패 - 런처를 다시 실행해주세요: {e}"))
}

/// `GGOLauncher.exe` → `GGOLauncher.<tag>.exe` (같은 폴더).
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("GGOLauncher");
    exe.with_file_name(format!("{stem}.{tag}.exe"))
}

/// rename 재시도(백신 검사 등으로 잠깐 잠긴 경우 대비, 최대 ~2초).
fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

/// 현재 exe를 old로 치우고 new를 그 자리에 둔다. 실패하면 원래대로 되돌린다.
fn swap_in(current: &Path, new: &Path, old: &Path) -> Result<(), String> {
    if old.exists() {
        std::fs::remove_file(old).ok();
    }
    rename_retry(current, old).map_err(|e| format!("기존 런처 이름 변경 실패: {e}"))?;
    if let Err(e) = rename_retry(new, current) {
        let _ = std::fs::rename(old, current);
        return Err(format!("새 런처 교체 실패 - 이전 버전을 유지합니다: {e}"));
    }
    Ok(())
}

/// 지난 업데이트가 남긴 .old.exe / .new.exe 정리. 직전 런처가 아직 종료 중이면
/// .old.exe가 잠겨 있을 수 있어 백그라운드에서 몇 번 다시 시도한다.
pub fn cleanup_leftovers() {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    let leftovers = [sibling(&current_exe, "old"), sibling(&current_exe, "new")];
    if !leftovers.iter().any(|p| p.exists()) {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..10 {
            for p in &leftovers {
                let _ = std::fs::remove_file(p);
            }
            if !leftovers.iter().any(|p| p.exists()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ggol-selfupd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sibling_names_stay_next_to_exe() {
        let exe = Path::new("C:/apps/GGOLauncher.exe");
        assert_eq!(sibling(exe, "new"), Path::new("C:/apps/GGOLauncher.new.exe"));
        assert_eq!(sibling(exe, "old"), Path::new("C:/apps/GGOLauncher.old.exe"));
    }

    #[test]
    fn swap_replaces_current_and_keeps_old() {
        let dir = scratch("swap");
        let cur = dir.join("GGOLauncher.exe");
        let (new, old) = (sibling(&cur, "new"), sibling(&cur, "old"));
        std::fs::write(&cur, "v1").unwrap();
        std::fs::write(&new, "v2").unwrap();
        std::fs::write(&old, "stale").unwrap();
        swap_in(&cur, &new, &old).unwrap();
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v2");
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "v1");
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_restores_current_when_new_is_missing() {
        let dir = scratch("restore");
        let cur = dir.join("GGOLauncher.exe");
        let (new, old) = (sibling(&cur, "new"), sibling(&cur, "old"));
        std::fs::write(&cur, "v1").unwrap();
        assert!(swap_in(&cur, &new, &old).is_err());
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "v1");
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
