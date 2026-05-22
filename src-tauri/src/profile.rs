use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub uo_path: String,
    #[serde(default)]
    pub cuo_path: Option<String>,
    pub server: ServerConfig,
    /// 자동 감지되거나 사용자가 직접 입력한 클라이언트 버전 (예: "7.0.95.0")
    #[serde(default)]
    pub client_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// DPAPI로 암호화된 비밀번호 (Phase 5에서 구현). 현재는 평문 placeholder.
    #[serde(default)]
    pub password_encrypted: String,
    #[serde(default)]
    pub encryption: EncryptionType,
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
