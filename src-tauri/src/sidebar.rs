//! 사이드바 링크 원격 fetch.
//!
//! ggoce-deploy의 `sidebar.json`에서 그룹/버튼 구조를 받아옴.
//! 차단·이전 시 런처 재배포 없이 URL 교체 가능.

use serde::{Deserialize, Serialize};

const SIDEBAR_URL: &str =
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/sidebar.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarLink {
    pub label: String,
    /// null이면 "준비중" 상태로 표시.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarGroup {
    pub label: String,
    pub buttons: Vec<SidebarLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidebar {
    pub groups: Vec<SidebarGroup>,
}

pub async fn fetch() -> Result<Sidebar, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(SIDEBAR_URL)
        .send()
        .await
        .map_err(|e| format!("sidebar 다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("sidebar HTTP {}", resp.status()));
    }
    resp.json::<Sidebar>()
        .await
        .map_err(|e| format!("sidebar JSON 파싱 실패: {e}"))
}
