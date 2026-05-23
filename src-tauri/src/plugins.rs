use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// `%APPDATA%\GGOLauncher\plugins\`
pub fn plugins_root() -> Result<PathBuf, String> {
    let base = dirs::data_dir().ok_or_else(|| "data_dir 없음".to_string())?;
    let dir = base.join("GGOLauncher").join("plugins");
    fs::create_dir_all(&dir).map_err(|e| format!("플러그인 폴더 생성 실패: {e}"))?;
    Ok(dir)
}

/// .dll 또는 .exe 단일 파일 선택. 사용자가 직접 둔 위치 그대로 사용 (복사 안 함).
pub async fn pick_plugin_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("플러그인 파일 선택 (.dll / .exe)")
        .add_filter("Plugin", &["dll", "exe"])
        .pick_file()
        .await
        .map(|h| h.path().to_string_lossy().into_owned())
}

/// .zip 선택 후 plugins/<name>/에 추출. 반환: 추출된 진입 .dll 절대 경로.
pub async fn import_from_zip() -> Result<Option<String>, String> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("플러그인 ZIP 선택")
        .add_filter("Plugin ZIP", &["zip"])
        .pick_file()
        .await;
    let Some(handle) = handle else { return Ok(None) };
    let zip_path = handle.path().to_path_buf();
    let name = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "ZIP 파일명 파싱 실패".to_string())?
        .to_string();

    let root = plugins_root()?;
    let target_dir = root.join(&name);
    if target_dir.exists() {
        // 이미 같은 이름 폴더 → 시간 suffix 추가
        let suffix = chrono_now_compact();
        let alt = root.join(format!("{name}_{suffix}"));
        return extract_and_locate(&zip_path, &alt);
    }
    extract_and_locate(&zip_path, &target_dir)
}

fn extract_and_locate(zip_path: &Path, target_dir: &Path) -> Result<Option<String>, String> {
    fs::create_dir_all(target_dir).map_err(|e| format!("디렉터리 생성 실패: {e}"))?;
    let file = fs::File::open(zip_path).map_err(|e| format!("ZIP 열기 실패: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("ZIP 파싱 실패: {e}"))?;
    let mut dll_candidates: Vec<PathBuf> = Vec::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP 엔트리 {i} 읽기 실패: {e}"))?;
        let Some(enclosed) = entry.enclosed_name() else { continue };
        let out_path = target_dir.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| format!("디렉터리 생성 실패: {e}"))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("부모 디렉터리 생성 실패: {e}"))?;
        }
        let mut out = fs::File::create(&out_path)
            .map_err(|e| format!("파일 쓰기 실패 {out_path:?}: {e}"))?;
        io::copy(&mut entry, &mut out)
            .map_err(|e| format!("복사 실패 {out_path:?}: {e}"))?;
        out.flush().ok();

        if out_path.extension().map(|x| x.to_ascii_lowercase()) == Some("dll".into()) {
            dll_candidates.push(out_path);
        }
    }

    if dll_candidates.is_empty() {
        return Err("ZIP 안에 .dll 파일을 찾지 못했습니다".into());
    }

    // 우선순위: 폴더명과 같은 이름의 DLL → 그 외 첫 DLL
    let folder_name = target_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let chosen = dll_candidates
        .iter()
        .find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase() == folder_name)
                .unwrap_or(false)
        })
        .cloned()
        .unwrap_or_else(|| dll_candidates[0].clone());

    Ok(Some(chosen.to_string_lossy().into_owned()))
}

fn chrono_now_compact() -> String {
    // 외부 chrono 의존 없이 epoch 초로 충돌 회피
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
