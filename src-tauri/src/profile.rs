use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub uo_path: String,
    #[serde(default)]
    pub cuo_path: Option<String>,
    pub server: ServerConfig,
    #[serde(default)]
    pub client_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,

    #[serde(default)]
    pub encryption: EncryptionType,

    /// 이 프로필의 계정 리스트. 여러 캐릭/계정을 한 프로필 안에 묶음.
    #[serde(default)]
    pub accounts: Vec<Account>,

    /// 현재 활성 계정 id (단일 PLAY 시 사용).
    #[serde(default)]
    pub active_account_id: Option<String>,

    // ── 레거시 필드 (구 settings.json 마이그레이션용) ──
    // load 시 자동으로 accounts[0]으로 이동, save 시 빈 값이면 안 씀.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password_encrypted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub username: String,
    /// DPAPI 암호화될 비밀번호 (현재는 평문 placeholder).
    pub password_encrypted: String,
    /// 선택 캐릭터명. 비어 있으면 기존처럼 서버 선택 단계까지만 자동 진행.
    #[serde(default)]
    pub character_name: Option<String>,
    /// 화면 표시용 별명 (예: "메인 캐릭"). 없으면 username 표시.
    #[serde(default)]
    pub display_name: Option<String>,
    /// MULTI LOGIN 대상 포함 여부. null/없음 = 레거시(아래 기본 정책 적용).
    /// 기본 정책: 프로필 내 첫 6개 계정 true, 7번째부터 false.
    #[serde(default)]
    pub multi_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionType {
    #[default]
    Auto,
    None,
    OldBlowfish,
    Blowfish125,
    Twofish,
}

/// 짧은 랜덤 id ("a_<base36 epoch>_<랜덤4>").
pub fn new_account_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 간단한 LCG로 4자리 base36 생성 (외부 rand crate 회피)
    let mut x: u64 = secs.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
    let mut suffix = String::new();
    for _ in 0..4 {
        let c = (x % 36) as u8;
        suffix.push(if c < 10 { (b'0' + c) as char } else { (b'a' + (c - 10)) as char });
        x /= 36;
    }
    format!("a_{:x}_{}", secs, suffix)
}
