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
    /// 화면 표시용 별명. 없으면 path의 파일명 사용.
    /// 같은 종류 다른 버전 플러그인 구분용.
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiSettings {
    #[serde(default = "default_lang")]
    pub language: String,
    /// 첫 사용 온보딩 배너 닫혔는지(영구). true면 다시 안 뜸.
    #[serde(default)]
    pub onboarding_dismissed: bool,
    /// PLAY/MULTI를 한 번이라도 실행했는지 — 온보딩 3단계 자동 체크용.
    #[serde(default)]
    pub first_launch_completed: bool,
    /// 설정 가이드를 한 번이라도 열었는지(영구). false면 사이드바 버튼 강조.
    #[serde(default)]
    pub guide_opened: bool,
    /// 레거시 메인 노출 순서. 마이그레이션 시 profiles 순서로 흡수하고 더는 저장하지 않음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_profile_ids: Option<Vec<String>>,
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
                character_name: None,
                display_name: None,
                multi_enabled: Some(true),
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

    // 과거 "메인 표시"로 선택한 프로필을 맨 앞으로 이동해 기존 사용자의 의도를 보존.
    // 이후부터는 profiles 배열 순서 자체가 메인 노출 우선순위다.
    if let Some(legacy_main_ids) = s.ui.main_profile_ids.take() {
        let order: std::collections::HashMap<String, usize> = legacy_main_ids
            .into_iter()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect();
        s.profiles
            .sort_by_key(|profile| order.get(&profile.id).copied().unwrap_or(usize::MAX));
    }

    let valid_ids: std::collections::HashSet<String> =
        s.profiles.iter().map(|p| p.id.clone()).collect();
    match &s.active_profile_id {
        Some(active) if !valid_ids.contains(active) => {
            s.active_profile_id = s.profiles.first().map(|profile| profile.id.clone());
        }
        None => {
            s.active_profile_id = s.profiles.first().map(|profile| profile.id.clone());
        }
        _ => {}
    }
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = settings_path()?;
    let json = serde_json::to_string_pretty(s).map_err(|e| format!("직렬화 실패: {e}"))?;
    fs::write(&p, json).map_err(|e| format!("settings 쓰기 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{EncryptionType, Profile, ServerConfig};

    fn profile(id: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            uo_path: String::new(),
            cuo_path: None,
            server: ServerConfig {
                address: "localhost".to_string(),
                port: 2593,
                encryption: EncryptionType::Auto,
                accounts: Vec::new(),
                active_account_id: None,
                username: String::new(),
                password_encrypted: String::new(),
            },
            client_version: None,
        }
    }

    #[test]
    fn legacy_main_profiles_become_profile_order() {
        let mut settings = Settings {
            active_profile_id: Some("c".to_string()),
            profiles: vec![profile("a"), profile("b"), profile("c")],
            plugins: Vec::new(),
            ui: UiSettings {
                main_profile_ids: Some(vec!["b".to_string(), "a".to_string()]),
                ..UiSettings::default()
            },
        };

        migrate(&mut settings);

        let ids: Vec<&str> = settings.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
        assert_eq!(settings.active_profile_id.as_deref(), Some("c"));
        assert!(settings.ui.main_profile_ids.is_none());
    }

    #[test]
    fn missing_active_profile_falls_back_to_first_profile() {
        let mut settings = Settings {
            active_profile_id: Some("missing".to_string()),
            profiles: vec![profile("a"), profile("b")],
            plugins: Vec::new(),
            ui: UiSettings::default(),
        };

        migrate(&mut settings);

        assert_eq!(settings.active_profile_id.as_deref(), Some("a"));
    }
}
