use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 런처 전역 설정. `%APPDATA%\GGOLauncher\settings.json`에 저장.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// 현재 활성 프로필 id. 없으면 None.
    #[serde(default)]
    pub active_profile_id: Option<String>,

    /// 모든 프로필.
    #[serde(default)]
    pub profiles: Vec<Profile>,

    /// 전역 공용 플러그인 (모든 프로필 공유). 메인 페이지에서 토글.
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,

    /// UI 환경 설정 (다국어 등 향후 확장 자리).
    #[serde(default)]
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// .dll / .exe / 추출된 ZIP 안의 entry .dll 경로
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiSettings {
    #[serde(default = "default_lang")]
    pub language: String,
}

fn default_lang() -> String {
    "ko".to_string()
}

/// settings.json 절대 경로 (디렉터리 없으면 생성).
pub fn settings_path() -> Result<PathBuf, String> {
    let base = dirs::data_dir().ok_or_else(|| "data_dir 없음".to_string())?;
    let dir = base.join("GGOLauncher");
    fs::create_dir_all(&dir).map_err(|e| format!("디렉터리 생성 실패: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load() -> Result<Settings, String> {
    let p = settings_path()?;
    if !p.exists() {
        return Ok(Settings::default());
    }
    let txt = fs::read_to_string(&p).map_err(|e| format!("settings 읽기 실패: {e}"))?;
    serde_json::from_str(&txt).map_err(|e| format!("settings 파싱 실패: {e}"))
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = settings_path()?;
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("직렬화 실패: {e}"))?;
    fs::write(&p, json).map_err(|e| format!("settings 쓰기 실패: {e}"))
}
