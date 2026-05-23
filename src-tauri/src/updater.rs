//! CUO 본체 자동 업데이트.
//!
//! manifest URL → 파일 목록 + SHA256 받아서 로컬 파일과 비교.
//! 다른 파일만 다운로드 후 백업+rename으로 교체. 실패 시 rollback.
//!
//! 안전망:
//!   - manifest에 없는 파일은 절대 건드리지 않음 (사용자 프로필/설정 보존)
//!   - manifest path는 sanitize 후 cuo_dir 밖으로 못 빠져나가게 강제
//!   - 교체 전 기존 파일을 .bak로 백업 → 중간 실패 시 모두 복구

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// manifest의 path를 안전한 상대 경로 컴포넌트 리스트로 변환.
/// 거부: 절대경로, 드라이브, Windows verbatim/UNC prefix, "..", 빈 세그먼트.
fn sanitize_manifest_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() {
        return Err("manifest path가 비어있음".into());
    }
    // 백슬래시 정규화 (manifest는 forward slash 권장이지만 호환)
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(format!("manifest path 절대경로 금지: {raw}"));
    }
    let p = PathBuf::from(&normalized);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => {
                let s = seg.to_string_lossy();
                if s.is_empty() || s == "." {
                    continue;
                }
                // 드라이브 letter (C:) 감지
                if s.len() == 2 && s.ends_with(':') {
                    return Err(format!("manifest path drive prefix 금지: {raw}"));
                }
                out.push(seg);
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(format!("manifest path '..' 금지: {raw}"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("manifest path root/prefix 금지: {raw}"));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(format!("manifest path 유효 세그먼트 없음: {raw}"));
    }
    Ok(out)
}

/// 최종 dest가 base 안에 있는지 확인 (canonicalize 기반, 미존재 파일이면 부모 기준).
fn assert_inside_base(base: &Path, dest: &Path) -> Result<(), String> {
    let base_real = base
        .canonicalize()
        .map_err(|e| format!("base canonicalize 실패: {e}"))?;
    let probe = if dest.exists() {
        dest.canonicalize()
            .map_err(|e| format!("dest canonicalize 실패: {e}"))?
    } else {
        let parent = dest.parent().ok_or_else(|| "dest 부모 없음".to_string())?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("부모 생성 실패 {parent:?}: {e}"))?;
        let parent_real = parent
            .canonicalize()
            .map_err(|e| format!("parent canonicalize 실패: {e}"))?;
        parent_real.join(dest.file_name().ok_or_else(|| "dest filename 없음".to_string())?)
    };
    if !probe.starts_with(&base_real) {
        return Err(format!("manifest path가 base 밖을 가리킴: {dest:?}"));
    }
    Ok(())
}

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
        let safe = sanitize_manifest_path(&f.path)?;
        let local_path = cuo_dir.join(&safe);
        // 미존재 파일도 base 검증 (부모만 canonicalize)
        if let Some(parent) = local_path.parent() {
            if parent.exists() {
                let base_real = cuo_dir
                    .canonicalize()
                    .map_err(|e| format!("base canonicalize 실패: {e}"))?;
                let parent_real = parent
                    .canonicalize()
                    .map_err(|e| format!("parent canonicalize 실패: {e}"))?;
                if !parent_real.starts_with(&base_real) {
                    return Err(format!("manifest path가 base 밖: {}", f.path));
                }
            }
        }
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

    // base_url HTTPS 강제 (manifest 변조 시 http로 downgrade되는 케이스 차단)
    if !check.manifest.base_url.to_ascii_lowercase().starts_with("https://") {
        return Err("manifest base_url은 https://여야 합니다".into());
    }

    // 1단계: 모든 파일을 .new로 다운로드 + 해시 검증
    let mut tmp_files: Vec<(PathBuf, PathBuf)> = Vec::new();
    for cf in &check.changed {
        let safe = sanitize_manifest_path(&cf.path)?;
        let dest = cuo_dir.join(&safe);

        // 부모 디렉터리 생성 + base 확인
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("디렉터리 생성 실패 {parent:?}: {e}"))?;
        }
        assert_inside_base(&cuo_dir, &dest)?;

        let tmp = dest.with_extension(format!(
            "{}.new",
            dest.extension().and_then(|s| s.to_str()).unwrap_or("")
        ));

        let url = format!("{}{}", check.manifest.base_url, cf.path);
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

        // 해시 검증
        let actual = sha256_file(&tmp)?;
        let expected = check
            .manifest
            .files
            .iter()
            .find(|f| f.path == cf.path)
            .map(|f| f.sha256.clone())
            .unwrap_or_default();
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!(
                "해시 불일치: {} (expected={expected}, got={actual})",
                cf.path
            ));
        }

        tmp_files.push((tmp, dest));
    }

    // 2단계: backup → rename, 중간 실패 시 rollback.
    // 기존 파일을 .bak로 rename(같은 디렉터리, atomic) 후 .new를 원본 자리로 rename.
    // 한 파일이라도 실패하면 backup된 모든 파일을 원위치로 복구하고 .new는 삭제.
    let mut applied: Vec<(PathBuf, Option<PathBuf>, PathBuf)> = Vec::new();
    // (dest, backup_path or None if 원본 없었음, tmp_new_path)
    for (tmp, dest) in &tmp_files {
        let backup = if dest.exists() {
            let b = dest.with_extension(format!(
                "{}.bak",
                dest.extension().and_then(|s| s.to_str()).unwrap_or("")
            ));
            let _ = std::fs::remove_file(&b);
            if let Err(e) = std::fs::rename(dest, &b) {
                rollback(&applied);
                cleanup_tmps(&tmp_files);
                return Err(format!(
                    "기존 파일 백업(rename) 실패 {dest:?} → {b:?}: {e} (CUO 실행 중?)"
                ));
            }
            Some(b)
        } else {
            None
        };
        if let Err(e) = std::fs::rename(tmp, dest) {
            // .new → dest 실패 → 방금 만든 backup 되돌리고 rollback
            if let Some(ref b) = backup {
                let _ = std::fs::rename(b, dest);
            }
            rollback(&applied);
            cleanup_tmps(&tmp_files);
            return Err(format!("rename 실패 {tmp:?} → {dest:?}: {e}"));
        }
        applied.push((dest.clone(), backup, tmp.clone()));
    }

    // 성공 시 백업 정리
    for (_, backup, _) in &applied {
        if let Some(b) = backup {
            let _ = std::fs::remove_file(b);
        }
    }
    Ok(())
}

/// applied에 기록된 (dest, backup, _) 페어를 원상복구.
fn rollback(applied: &[(PathBuf, Option<PathBuf>, PathBuf)]) {
    for (dest, backup, _) in applied.iter().rev() {
        // 새로 들어간 파일 제거
        let _ = std::fs::remove_file(dest);
        // backup 있으면 되돌리기
        if let Some(b) = backup {
            let _ = std::fs::rename(b, dest);
        }
    }
}

fn cleanup_tmps(tmps: &[(PathBuf, PathBuf)]) {
    for (tmp, _) in tmps {
        let _ = std::fs::remove_file(tmp);
    }
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
