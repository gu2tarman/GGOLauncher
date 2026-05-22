use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
