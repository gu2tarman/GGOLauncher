//! 설치 자동 탐색 — UO 클라이언트 폴더, ClassicUO, 플러그인(RazorEnhanced / ClassicAssist).
//! 찾은 결과는 후보일 뿐이며, 등록 여부는 사용자가 고른다.

use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::paths;

#[derive(Debug, Clone, Serialize)]
pub struct FoundPlugin {
    pub path: String,
    /// "re" | "ca" | "other" (CUO settings.json에만 등록돼 있던 기타 플러그인)
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FoundCuo {
    pub path: String,
    /// version.txt가 있는 GGO CE 폴더인가 (아니면 원본 ClassicUO)
    pub ggoce: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Discovery {
    pub uo_folders: Vec<String>,
    pub cuo_folders: Vec<FoundCuo>,
    pub plugins: Vec<FoundPlugin>,
}

/// EA 설치기 레지스트리 키. 값 이름은 `InstallDir`.
const EA_REG_KEYS: [&str; 2] = [
    r"SOFTWARE\WOW6432Node\Electronic Arts\EA Games\Ultima Online Classic",
    r"SOFTWARE\Electronic Arts\EA Games\Ultima Online Classic",
];

const DEFAULT_UO_DIRS: [&str; 3] = [
    r"C:\Program Files (x86)\Electronic Arts\Ultima Online Classic",
    r"C:\Program Files\Electronic Arts\Ultima Online Classic",
    r"C:\Ultima Online Classic",
];

/// 폴더 훑기에서 들어가지 않는 폴더 (소문자).
const SKIP_DIRS: [&str; 12] = [
    "windows",
    "$recycle.bin",
    "system volume information",
    "programdata",
    "appdata",
    "node_modules",
    ".git",
    "winsxs",
    "recovery",
    "perflogs",
    "msocache",
    "$windows.~bt",
];

const SCAN_DEADLINE: Duration = Duration::from_secs(6);
const SCAN_DIR_BUDGET: usize = 60_000;

/// 빠른 UO 폴더 탐색 (레지스트리 + 기본 설치 경로). 프로필 편집 창 자동 채우기용.
pub fn find_uo_folder() -> Option<String> {
    quick_uo_candidates().into_iter().next()
}

fn quick_uo_candidates() -> Vec<String> {
    let mut out = Vec::new();
    for key in EA_REG_KEYS {
        if let Some(dir) = read_hklm_string(key, "InstallDir") {
            out.push(dir);
        }
    }
    out.extend(DEFAULT_UO_DIRS.iter().map(|s| s.to_string()));
    out.into_iter().filter(|p| is_uo_dir(Path::new(p))).collect()
}

/// 전체 탐색. 흔한 위치를 깊이·시간 제한 안에서 훑고, 찾은 ClassicUO의 settings.json에서
/// UO 경로와 플러그인 경로를 추가로 읽는다.
pub fn scan() -> Discovery {
    let mut uo = Ordered::default();
    let mut cuo = Ordered::default();
    let mut plugins: Vec<FoundPlugin> = Vec::new();
    let mut plugin_seen = HashSet::new();
    let mut add_plugin = |path: &Path, plugins: &mut Vec<FoundPlugin>| {
        let Some(kind) = plugin_kind(path) else { return };
        if !path.is_file() || !plugin_seen.insert(key_of(path)) {
            return;
        }
        plugins.push(FoundPlugin {
            path: path.to_string_lossy().into_owned(),
            kind: kind.to_string(),
        });
    };

    for dir in quick_uo_candidates() {
        uo.push(PathBuf::from(dir));
    }

    let deadline = Instant::now() + SCAN_DEADLINE;
    let mut budget = SCAN_DIR_BUDGET;
    let mut visited = HashMap::new();
    let fixed = fixed_drives();
    for (root, depth) in scan_roots(&fixed) {
        walk(&root, depth, deadline, &mut budget, &mut visited, &mut |dir, files| {
            let has = |name: &str| files.contains(name);
            if has("classicuo.exe") {
                cuo.push(dir.to_path_buf());
            }
            if has("client.exe") && is_uo_dir(dir) {
                uo.push(dir.to_path_buf());
            }
            for name in ["razorenhanced.exe", "classicassist.dll"] {
                if has(name) {
                    if let Some(actual) = actual_file(dir, name) {
                        add_plugin(&actual, &mut plugins);
                    }
                }
            }
        });
    }

    // ClassicUO settings.json: ultimaonlinedirectory + plugins (상대 경로는 CUO 폴더 기준)
    // 네트워크 경로는 접근만으로 수십 초 멈출 수 있어 로컬 고정 디스크 경로만 확인한다.
    for cuo_dir in cuo.items.clone() {
        if Instant::now() >= deadline {
            break;
        }
        let Some(json) = read_settings_json(&cuo_dir) else { continue };
        if let Some(dir) = json.get("ultimaonlinedirectory").and_then(|v| v.as_str()) {
            let p = PathBuf::from(dir);
            if is_local(&p, &fixed) && is_uo_dir(&p) {
                uo.push(p);
            }
        }
        if let Some(list) = json.get("plugins").and_then(|v| v.as_array()) {
            for entry in list.iter().filter_map(|v| v.as_str()) {
                let p = PathBuf::from(entry);
                let p = if p.is_absolute() { p } else { cuo_dir.join(p) };
                if is_local(&p, &fixed) {
                    add_plugin(&normalize(&p), &mut plugins);
                }
            }
        }
    }

    Discovery {
        uo_folders: uo.strings(),
        cuo_folders: cuo
            .strings()
            .into_iter()
            .map(|path| FoundCuo {
                ggoce: matches!(paths::detect_folder_kind(&path), paths::FolderKind::Ggoce { .. }),
                path,
            })
            .collect(),
        plugins,
    }
}

fn scan_roots(fixed: &[PathBuf]) -> Vec<(PathBuf, usize)> {
    let mut roots = Vec::new();
    if let Some(dir) = paths::launcher_dir() {
        roots.push((PathBuf::from(dir), 3));
    }
    if let Some(dir) = dirs::data_dir() {
        roots.push((dir.join("GGOLauncher"), 4));
    }
    for dir in [dirs::desktop_dir(), dirs::download_dir(), dirs::document_dir()]
        .into_iter()
        .flatten()
    {
        roots.push((dir, 3));
    }
    if let Some(dir) = dirs::home_dir() {
        roots.push((dir, 2));
    }
    for drive in fixed {
        roots.push((drive.clone(), 3));
    }
    // 네트워크로 리디렉션된 문서·바탕화면 폴더 등은 제외
    roots.retain(|(root, _)| is_local(root, fixed));
    roots
}

/// 로컬 고정 디스크 위의 경로인가 (UNC·네트워크·이동식 드라이브 제외).
fn is_local(path: &Path, fixed: &[PathBuf]) -> bool {
    if cfg!(not(windows)) {
        return true;
    }
    let s = path.to_string_lossy();
    if s.starts_with(r"\\") || s.starts_with("//") {
        return false;
    }
    let Some(letter) = s.chars().next().filter(|_| s.chars().nth(1) == Some(':')) else {
        return false;
    };
    fixed
        .iter()
        .any(|d| d.to_string_lossy().chars().next().map(|c| c.eq_ignore_ascii_case(&letter)) == Some(true))
}

/// BFS로 depth까지 내려가며 각 폴더의 파일 이름(소문자) 집합을 콜백에 넘긴다.
/// visited는 폴더별로 이미 훑은 "남은 깊이"를 기록 — 다른 루트에서 더 깊게 볼 수 있으면 다시 본다.
fn walk(
    root: &Path,
    max_depth: usize,
    deadline: Instant,
    budget: &mut usize,
    visited: &mut HashMap<String, usize>,
    on_dir: &mut dyn FnMut(&Path, &HashSet<String>),
) {
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if *budget == 0 || Instant::now() >= deadline {
            return;
        }
        let remaining = max_depth - depth;
        let key = key_of(&dir);
        if visited.get(&key).is_some_and(|&seen| seen >= remaining) {
            continue;
        }
        visited.insert(key, remaining);
        *budget -= 1;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut files = HashSet::new();
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if ft.is_file() {
                files.insert(name);
            } else if ft.is_dir() && depth < max_depth && !SKIP_DIRS.contains(&name.as_str()) {
                queue.push_back((entry.path(), depth + 1));
            }
        }
        on_dir(&dir, &files);
    }
}

fn plugin_kind(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    match name.as_str() {
        "razorenhanced.exe" => Some("re"),
        "classicassist.dll" => Some("ca"),
        _ if name.ends_with(".dll") || name.ends_with(".exe") => Some("other"),
        _ => None,
    }
}

/// paths::inspect의 valid_uo는 client.exe만 있어도 통과해 다른 게임 폴더가 잡힌다.
/// 탐색에서는 UO 데이터 파일(tiledata.mul)까지 함께 있어야 인정한다.
fn is_uo_dir(dir: &Path) -> bool {
    dir.join("client.exe").is_file() && dir.join("tiledata.mul").is_file()
}

/// 대소문자가 다른 실제 파일명으로 경로를 만든다 (표시용).
fn actual_file(dir: &Path, lower_name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().to_lowercase() == lower_name)
        .map(|e| e.path())
}

fn read_settings_json(cuo_dir: &Path) -> Option<serde_json::Value> {
    let path = cuo_dir.join("settings.json");
    if std::fs::metadata(&path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// `a\.\b\..\c` 같은 상대 표기를 정리한다 (파일 존재 여부와 무관).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn key_of(p: &Path) -> String {
    p.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_lowercase()
}

/// 넣은 순서를 지키는 중복 제거 목록.
#[derive(Default)]
struct Ordered {
    seen: HashSet<String>,
    items: Vec<PathBuf>,
}

impl Ordered {
    fn push(&mut self, p: PathBuf) {
        if self.seen.insert(key_of(&p)) {
            self.items.push(p);
        }
    }
    fn strings(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn read_hklm_string(subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let subkey = wide(subkey);
    let value = wide(value);
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(windows))]
fn read_hklm_string(_subkey: &str, _value: &str) -> Option<String> {
    None
}

/// 고정 디스크 드라이브 루트 (네트워크·이동식 드라이브는 느릴 수 있어 제외).
#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| unsafe { GetDriveTypeW(wide(root).as_ptr()) } == DRIVE_FIXED)
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
fn fixed_drives() -> Vec<PathBuf> {
    Vec::new()
}
