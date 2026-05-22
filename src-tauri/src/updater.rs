//! CUO 본체 자동 업데이트.
//!
//! manifest URL → 파일 목록 + SHA256 받아서 로컬 파일과 비교.
//! 다른 파일만 다운로드 후 atomic rename으로 교체.
//!
//! 안전망: manifest에 없는 파일은 절대 건드리지 않음 (사용자 프로필/설정 보존).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/client/manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub released: String,
    #[serde(default)]
    pub notes: String,
    pub base_url: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// 업데이트 체크 결과.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheck {
    /// 원격 manifest의 버전.
    pub remote_version: String,
    /// 현재 로컬 GGOCE 버전 (cuo.dll PE에서 감지).
    pub local_version: Option<String>,
    /// 변경/누락된 파일 목록 (path + size + 이유).
    pub changed: Vec<ChangedFile>,
    /// 총 다운로드 바이트.
    pub total_bytes: u64,
    /// manifest 그대로 보존 (apply 시 다시 다운로드 안 받게).
    pub manifest: UpdateManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    pub size: u64,
    pub reason: ChangeReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReason {
    Missing,
    SizeMismatch,
    HashMismatch,
}

/// manifest 다운로드 → 로컬 파일 해시 비교 → 변경 파일 목록 반환.
pub async fn check_update(cuo_dir: &str) -> Result<UpdateCheck, String> {
    let manifest = fetch_manifest().await?;
    let cuo_dir = PathBuf::from(cuo_dir);
    if !cuo_dir.is_dir() {
        return Err(format!("CUO 경로가 폴더가 아님: {}", cuo_dir.display()));
    }

    let local_version = crate::paths::detect_ggoce_version(&cuo_dir.to_string_lossy());

    let mut changed = Vec::new();
    let mut total_bytes = 0u64;
    for f in &manifest.files {
        let local_path = cuo_dir.join(&f.path);
        match check_file(&local_path, f) {
            Ok(None) => continue, // 이미 일치 → 스킵
            Ok(Some(reason)) => {
                total_bytes += f.size;
                changed.push(ChangedFile {
                    path: f.path.clone(),
                    size: f.size,
                    reason,
                });
            }
            Err(_) => {
                // 읽기 실패 → 다운로드 대상으로 간주
                total_bytes += f.size;
                changed.push(ChangedFile {
                    path: f.path.clone(),
                    size: f.size,
                    reason: ChangeReason::Missing,
                });
            }
        }
    }

    Ok(UpdateCheck {
        remote_version: manifest.version.clone(),
        local_version,
        changed,
        total_bytes,
        manifest,
    })
}

/// 단일 파일 검사. Ok(None) = 일치, Ok(Some(reason)) = 변경 필요.
fn check_file(local: &Path, m: &ManifestFile) -> Result<Option<ChangeReason>, String> {
    if !local.exists() {
        return Ok(Some(ChangeReason::Missing));
    }
    let meta = std::fs::metadata(local).map_err(|e| e.to_string())?;
    if meta.len() != m.size {
        return Ok(Some(ChangeReason::SizeMismatch));
    }
    let hash = sha256_file(local)?;
    if !hash.eq_ignore_ascii_case(&m.sha256) {
        return Ok(Some(ChangeReason::HashMismatch));
    }
    Ok(None)
}

/// 파일 SHA256 → hex 소문자.
fn sha256_file(path: &Path) -> Result<String, String> {
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

/// 다운로드 + atomic apply. 진행률은 on_progress(bytes_done, total)로 콜백.
pub async fn apply_update<F>(
    cuo_dir: &str,
    check: &UpdateCheck,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    use futures_util::StreamExt;

    let cuo_dir = PathBuf::from(cuo_dir);
    if !cuo_dir.is_dir() {
        return Err(format!("CUO 경로가 폴더가 아님: {}", cuo_dir.display()));
    }

    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let total_bytes = check.total_bytes;
    let mut bytes_done: u64 = 0;
    on_progress(0, total_bytes);

    // 1단계: 모든 파일을 .new로 다운로드
    let mut tmp_files: Vec<(PathBuf, PathBuf)> = Vec::new();
    for cf in &check.changed {
        let url = format!("{}{}", check.manifest.base_url, cf.path);
        let dest = cuo_dir.join(&cf.path);
        let tmp = dest.with_extension(format!(
            "{}.new",
            dest.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
        ));

        // 부모 디렉터리 생성 (manifest에 새 서브폴더 파일 추가 시 대비)
        if let Some(parent) = tmp.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("디렉터리 생성 실패 {parent:?}: {e}"))?;
        }

        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("다운로드 실패 {url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} ({url})", resp.status()));
        }

        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("임시 파일 생성 실패 {tmp:?}: {e}"))?;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| format!("스트림 읽기 실패: {e}"))?;
            tokio::io::AsyncWriteExt::write_all(&mut file, &bytes)
                .await
                .map_err(|e| format!("쓰기 실패: {e}"))?;
            bytes_done = bytes_done.saturating_add(bytes.len() as u64);
            on_progress(bytes_done, total_bytes);
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| format!("flush 실패: {e}"))?;
        drop(file);

        // 다운로드된 임시 파일의 해시 검증
        let actual = sha256_file(&tmp)?;
        let expected = check
            .manifest
            .files
            .iter()
            .find(|f| f.path == cf.path)
            .map(|f| f.sha256.clone())
            .unwrap_or_default();
        if !actual.eq_ignore_ascii_case(&expected) {
            // 검증 실패 → 임시 파일 삭제, 에러
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "해시 불일치: {} (expected={expected}, got={actual})",
                cf.path
            ));
        }

        tmp_files.push((tmp, dest));
    }

    // 2단계: 모든 다운로드 성공 시 .new → 원본 atomic rename
    for (tmp, dest) in &tmp_files {
        // 원본이 있으면 백업 (rollback용은 아니고, Windows에서 사용중일 때 회피)
        // 단순화: 그냥 replace. Windows에서 사용 중인 파일은 rename 실패.
        // CUO가 실행 중이면 사용자가 닫고 다시 시도해야 함.
        if dest.exists() {
            std::fs::remove_file(dest).map_err(|e| {
                format!(
                    "기존 파일 제거 실패 {dest:?}: {e} (CUO가 실행 중일 수 있음)"
                )
            })?;
        }
        std::fs::rename(tmp, dest)
            .map_err(|e| format!("rename 실패 {tmp:?} → {dest:?}: {e}"))?;
    }

    Ok(())
}

async fn fetch_manifest() -> Result<UpdateManifest, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(MANIFEST_URL)
        .send()
        .await
        .map_err(|e| format!("manifest 다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("manifest HTTP {}", resp.status()));
    }
    resp.json::<UpdateManifest>()
        .await
        .map_err(|e| format!("manifest JSON 파싱 실패: {e}"))
}
