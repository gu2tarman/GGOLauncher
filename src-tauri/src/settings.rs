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
    let mut s: Settings =
        serde_json::from_str(&txt).map_err(|e| format!("settings 파싱 실패: {e}"))?;
    migrate(&mut s);
    Ok(s)
}

/// 구 데이터 → 신규 계정 리스트 구조로 자동 이전.
/// - profile.server.username/password_encrypted 가 있고 accounts가 비어있으면 첫 Account로 이동.
fn migrate(s: &mut Settings) {
    use crate::profile::{new_account_id, Account};
    for p in &mut s.profiles {
        let has_legacy = !p.server.username.is_empty() || !p.server.password_encrypted.is_empty();
        if has_legacy && p.server.accounts.is_empty() {
            let id = new_account_id();
            p.server.accounts.push(Account {
                id: id.clone(),
                username: std::mem::take(&mut p.server.username),
                password_encrypted: std::mem::take(&mut p.server.password_encrypted),
                display_name: None,
            });
            p.server.active_account_id = Some(id);
        }
        // active_account_id가 가리키는 계정이 사라졌으면 첫 번째로 대체
        if let Some(active) = &p.server.active_account_id {
            if !p.server.accounts.iter().any(|a| &a.id == active) {
                p.server.active_account_id = p.server.accounts.first().map(|a| a.id.clone());
            }
        } else if !p.server.accounts.is_empty() {
            p.server.active_account_id = Some(p.server.accounts[0].id.clone());
        }
    }
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = settings_path()?;
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("직렬화 실패: {e}"))?;
    fs::write(&p, json).map_err(|e| format!("settings 쓰기 실패: {e}"))
}
