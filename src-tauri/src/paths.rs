use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathInfo {
    pub exists: bool,
    pub is_dir: bool,
    pub is_file: bool,
    /// UO 클라이언트 폴더로 추정 가능한가 (client.exe / anim.idx 등 존재).
    pub valid_uo: bool,
    /// ClassicUO 폴더로 추정 가능한가 (ClassicUO.exe 존재).
    pub valid_cuo: bool,
}

pub fn inspect(path: &str) -> PathInfo {
    let p = PathBuf::from(path);
    let exists = p.exists();
    let is_dir = p.is_dir();
    let is_file = p.is_file();

    let valid_uo = is_dir
        && (p.join("client.exe").exists()
            || p.join("Client.exe").exists()
            || p.join("anim.idx").exists()
            || p.join("art.mul").exists());

    let valid_cuo = is_dir
        && (p.join("ClassicUO.exe").exists() || p.join("classicuo.exe").exists());

    PathInfo {
        exists,
        is_dir,
        is_file,
        valid_uo,
        valid_cuo,
    }
}

/// UO 폴더에서 client.exe 찾아 파일 버전 감지.
/// Windows의 VS_FIXEDFILEINFO를 PowerShell로 읽음 (간단 + 의존성 최소).
/// 형식 예: "7.0.95.0"
pub fn detect_client_version(uo_path: &str) -> Option<String> {
    let dir = PathBuf::from(uo_path);
    if !dir.is_dir() {
        return None;
    }
    let candidates = ["client.exe", "Client.exe", "CLIENT.EXE"];
    let client_exe = candidates
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())?;
    read_pe_file_version(&client_exe)
}

/// CUO 폴더에서 ClassicUO.exe 찾아 파일 버전 감지.
/// GGO CE는 ClassicUO 포크라 PE 메타데이터에 빌드 버전 박혀있음.
pub fn detect_ggoce_version(cuo_path: &str) -> Option<String> {
    let dir = PathBuf::from(cuo_path);
    if !dir.is_dir() {
        return None;
    }
    let candidates = ["ClassicUO.exe", "classicuo.exe", "CLASSICUO.EXE"];
    let exe = candidates
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())?;
    read_pe_file_version(&exe)
}

#[cfg(target_os = "windows")]
fn read_pe_file_version(exe: &Path) -> Option<String> {
    let path_str = exe.to_string_lossy();
    // FileVersion 문자열은 옛 포맷("7, 0, 114, 65")일 수 있음.
    // FileMajor/Minor/Build/Private 정수로 직접 조합해 "7.0.114.65" 강제.
    let script = format!(
        r#"$v = (Get-Item -LiteralPath "{}").VersionInfo; "$($v.FileMajorPart).$($v.FileMinorPart).$($v.FileBuildPart).$($v.FilePrivatePart)""#,
        path_str.replace('"', "`\"")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s == "..." || s == "0.0.0.0" || s.to_lowercase().contains("error") {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "windows"))]
fn read_pe_file_version(_exe: &Path) -> Option<String> {
    None
}

/// 네이티브 폴더 선택 다이얼로그.
/// start_dir이 유효 폴더면 거기서 시작, 아니면 기본 위치.
pub async fn pick_folder(start_dir: Option<String>, title: &str) -> Option<String> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if let Some(d) = start_dir.filter(|d| !d.is_empty() && std::path::Path::new(d).is_dir()) {
        dialog = dialog.set_directory(d);
    }
    dialog
        .pick_folder()
        .await
        .map(|h| h.path().to_string_lossy().into_owned())
}
