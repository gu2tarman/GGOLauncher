use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// 폴더가 어떤 종류인지 — 자동 업데이트 전 안전성 판정용.
/// 마커: `version.txt`(GGO CE 배포가 항상 포함하는 파일)의 존재 유무.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FolderKind {
    /// 폴더 없음/비어있음(.bak/.new 제외 시 빈 상태 포함). 신규 설치 대상.
    NewInstall,
    /// version.txt 발견 → GGO CE 확정.
    Ggoce { version: String },
    /// ClassicUO.exe는 있지만 version.txt 없음 → 원본 CUO(또는 비-GGO CE 변형).
    OriginalCuo,
    /// 위 모든 경우에 해당 안 됨 (파일은 있지만 식별 불가).
    Unknown,
}

/// 폴더 종류 판정. 빈/미존재 폴더는 NewInstall.
pub fn detect_folder_kind(path: &str) -> FolderKind {
    let p = PathBuf::from(path);
    if !p.is_dir() {
        return FolderKind::NewInstall;
    }
    // GGO CE 마커: version.txt
    let vtxt = p.join("version.txt");
    if vtxt.is_file() {
        if let Ok(s) = std::fs::read_to_string(&vtxt) {
            let v = s.lines().next().unwrap_or("").trim().to_string();
            if !v.is_empty() {
                return FolderKind::Ggoce { version: v };
            }
        }
        return FolderKind::Ggoce { version: String::new() };
    }
    // ClassicUO.exe만 있고 마커 없음 → 원본 CUO
    let has_cuo_exe = ["ClassicUO.exe", "classicuo.exe", "CLASSICUO.EXE"]
        .iter()
        .any(|n| p.join(n).is_file());
    if has_cuo_exe {
        return FolderKind::OriginalCuo;
    }
    // 비어있는지 (사용자 데이터 흔적 없음) 판단
    let empty = std::fs::read_dir(&p)
        .map(|mut it| it.next().is_none())
        .unwrap_or(false);
    if empty {
        FolderKind::NewInstall
    } else {
        FolderKind::Unknown
    }
}

/// 런처 exe가 위치한 디렉터리 경로 반환. 신규 설치 기본 위치 계산용.
pub fn launcher_dir() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
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

/// GGO CE 버전 감지. 우선순위:
///   1) version.txt 사이드 파일 (빌드가 두면 사용)
///   2) cuo.dll PE FileVersion (GGO CE 본체 빌드가 직접 박는 값)
///   3) ClassicUO.exe PE FileVersion (upstream 폴백)
pub fn detect_ggoce_version(cuo_path: &str) -> Option<String> {
    let dir = PathBuf::from(cuo_path);
    if !dir.is_dir() {
        return None;
    }

    // 1순위: version.txt
    let vtxt = dir.join("version.txt");
    if vtxt.exists() {
        if let Ok(s) = std::fs::read_to_string(&vtxt) {
            let v = s.lines().next().unwrap_or("").trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }

    // 2순위: cuo.dll PE 버전 (GGO CE가 직접 새기는 곳)
    for name in ["cuo.dll", "CUO.dll", "CUO.DLL"] {
        let p = dir.join(name);
        if p.exists() {
            if let Some(v) = read_pe_file_version(&p) {
                if !is_default_version(&v) {
                    return Some(v);
                }
            }
        }
    }

    // 3순위: ClassicUO.exe PE 버전 (upstream)
    for name in ["ClassicUO.exe", "classicuo.exe", "CLASSICUO.EXE"] {
        let p = dir.join(name);
        if p.exists() {
            return read_pe_file_version(&p);
        }
    }

    None
}

/// "0.0.0.0" 같은 의미 없는 기본값 필터.
fn is_default_version(v: &str) -> bool {
    matches!(v, "0.0.0.0" | "0.0.0" | "0.0")
}

#[cfg(target_os = "windows")]
fn read_pe_file_version(exe: &Path) -> Option<String> {
    // Win32 API 직접 호출 — PowerShell spawn (300~500ms + 콘솔 깜빡임) 대체.
    // 우선순위: FileVersion (0.0.0.0 아니면 채택) → ProductVersion 폴백
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(once(0)).collect()
    }

    let exe_w = to_wide(&exe.to_string_lossy());

    // 1. Version info 데이터 크기 조회
    let mut dummy: u32 = 0;
    let size = unsafe { GetFileVersionInfoSizeW(exe_w.as_ptr(), &mut dummy) };
    if size == 0 {
        return None;
    }

    // 2. Version info 버퍼 채우기
    let mut buf: Vec<u8> = vec![0; size as usize];
    let ok =
        unsafe { GetFileVersionInfoW(exe_w.as_ptr(), 0, size, buf.as_mut_ptr() as *mut _) };
    if ok == 0 {
        return None;
    }

    // 3. "\\" 서브블록에서 VS_FIXEDFILEINFO 추출
    let sub_block = to_wide("\\");
    let mut info_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut info_len: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const _,
            sub_block.as_ptr(),
            &mut info_ptr,
            &mut info_len,
        )
    };
    if ok == 0 || info_ptr.is_null() {
        return None;
    }
    if (info_len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }
    let info = unsafe { &*(info_ptr as *const VS_FIXEDFILEINFO) };
    if info.dwSignature != 0xFEEF_04BD {
        return None;
    }
    // FileVersion 시도
    let file_ver = format_version(info.dwFileVersionMS, info.dwFileVersionLS);
    if file_ver != "0.0.0.0" {
        return Some(file_ver);
    }
    // 폴백: ProductVersion (FileVersion이 0.0.0.0인 빌드 케이스)
    let prod_ver = format_version(info.dwProductVersionMS, info.dwProductVersionLS);
    if prod_ver != "0.0.0.0" {
        return Some(prod_ver);
    }
    None
}

#[cfg(target_os = "windows")]
fn format_version(ms: u32, ls: u32) -> String {
    let major = (ms >> 16) & 0xFFFF;
    let minor = ms & 0xFFFF;
    let build = (ls >> 16) & 0xFFFF;
    let rev = ls & 0xFFFF;
    format!("{major}.{minor}.{build}.{rev}")
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
