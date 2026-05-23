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

#[derive(Debug, Clone)]
struct SafeManifestPath {
    fs_path: PathBuf,
    url_path: String,
}

/// manifest의 path를 안전한 상대 경로로 변환.
/// 거부: 절대경로, 드라이브/UNC prefix, "..", 빈 세그먼트, URL/경로 혼동 문자.
fn sanitize_manifest_path(raw: &str) -> Result<SafeManifestPath, String> {
    if raw.is_empty() {
        return Err("manifest path가 비어있음".into());
    }
    // 백슬래시 정규화 (manifest는 forward slash 권장이지만 호환)
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(format!("manifest path 절대경로 금지: {raw}"));
    }
    let p = PathBuf::from(&normalized);
    let mut fs_path = PathBuf::new();
    let mut url_parts: Vec<String> = Vec::new();
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
                if !s.chars().all(is_safe_manifest_path_char) {
                    return Err(format!("manifest path 허용되지 않는 문자: {raw}"));
                }
                fs_path.push(seg);
                url_parts.push(s.into_owned());
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
    if fs_path.as_os_str().is_empty() {
        return Err(format!("manifest path 유효 세그먼트 없음: {raw}"));
    }
    Ok(SafeManifestPath {
        fs_path,
        url_path: url_parts.join("/"),
    })
}

fn is_safe_manifest_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

fn build_manifest_file_url(base_url: &str, safe_path: &SafeManifestPath) -> Result<String, String> {
    let mut base =
        reqwest::Url::parse(base_url).map_err(|e| format!("manifest base_url 오류: {e}"))?;
    if base.scheme() != "https" {
        return Err("manifest base_url은 https://여야 합니다".into());
    }
    if base.host_str().is_none() {
        return Err("manifest base_url에는 host가 필요합니다".into());
    }
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base.join(&safe_path.url_path)
        .map(|u| u.to_string())
        .map_err(|e| format!("manifest 파일 URL 조합 실패: {e}"))
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
        std::fs::create_dir_all(parent).map_err(|e| format!("부모 생성 실패 {parent:?}: {e}"))?;
        let parent_real = parent
            .canonicalize()
            .map_err(|e| format!("parent canonicalize 실패: {e}"))?;
        parent_real.join(
            dest.file_name()
                .ok_or_else(|| "dest filename 없음".to_string())?,
        )
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
        let local_path = cuo_dir.join(&safe.fs_path);
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
///
/// `allow_original_overwrite`: cuo_dir이 원본 CUO 폴더로 감지된 경우에만 의미.
/// false면 원본 폴더 덮어쓰기를 거부한다 (IPC 오용 방어).
/// UI는 사용자 확인을 거친 뒤에만 true로 호출한다.
pub async fn apply_update<F>(
    cuo_dir: &str,
    check: &UpdateCheck,
    allow_original_overwrite: bool,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    use futures_util::StreamExt;

    // 폴더가 없으면 생성 (신규 설치 흐름 지원).
    let cuo_dir = PathBuf::from(cuo_dir);
    if !cuo_dir.exists() {
        std::fs::create_dir_all(&cuo_dir)
            .map_err(|e| format!("CUO 폴더 생성 실패 {}: {e}", cuo_dir.display()))?;
    }
    if !cuo_dir.is_dir() {
        return Err(format!("CUO 경로가 폴더가 아님: {}", cuo_dir.display()));
    }

    // 백엔드 가드: 원본 CUO 폴더로 판정되면 명시적 허용 없이는 거부.
    if !allow_original_overwrite {
        if let crate::paths::FolderKind::OriginalCuo =
            crate::paths::detect_folder_kind(&cuo_dir.to_string_lossy())
        {
            return Err(
                "ORIGINAL_CUO_BLOCKED: 원본 ClassicUO 폴더로 감지됨. \
                 UI 확인을 거친 뒤 다시 시도하세요."
                    .into(),
            );
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let total_bytes = check.total_bytes;
    let mut bytes_done: u64 = 0;
    on_progress(0, total_bytes);

    // 1단계: 모든 파일을 .new로 다운로드 + 해시 검증
    let mut tmp_files: Vec<(PathBuf, PathBuf)> = Vec::new();
    for cf in &check.changed {
        let safe = sanitize_manifest_path(&cf.path)?;
        let dest = cuo_dir.join(&safe.fs_path);

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

        let url = build_manifest_file_url(&check.manifest.base_url, &safe)?;
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

/// 신규 설치용: manifest만 받아서 모든 파일을 Missing으로 표시.
/// cuo_path가 비었거나 폴더가 아직 없을 때 호출.
pub async fn fetch_manifest_as_install() -> Result<UpdateCheck, String> {
    let manifest = fetch_manifest().await?;
    let mut changed = Vec::with_capacity(manifest.files.len());
    let mut total_bytes = 0u64;
    for f in &manifest.files {
        // 경로 sanitize는 apply 시점에서도 다시 함 — 여기서도 미리 검증해 일관성 확보.
        let _ = sanitize_manifest_path(&f.path)?;
        total_bytes += f.size;
        changed.push(ChangedFile {
            path: f.path.clone(),
            size: f.size,
            reason: ChangeReason::Missing,
        });
    }
    Ok(UpdateCheck {
        remote_version: manifest.version.clone(),
        local_version: None,
        changed,
        total_bytes,
        manifest,
    })
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
