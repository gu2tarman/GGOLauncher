//! UO 챔피언 서바이버 명예의 전당 fetch (사이드바 카드 1위 순환 + 명예의 전당 모달).
//!
//! URL은 고정 상수 — sidebar.json 같은 원격 설정으로 바꿀 수 없게 해서
//! 비상 채널(사이드바)이 임의 주소를 읽게 되는 일을 막는다.
//! 응답 형태는 서바이버 `functions/api/public/hall.ts`가 정하고, 여기선 JSON을 그대로 넘긴다.

const HALL_URL: &str = "https://uo-survivor.pages.dev/api/public/hall";

fn hall_url() -> String {
    // 개발 빌드에서만 로컬 서버(wrangler pages dev)로 바꿔 볼 수 있음. 릴리스 빌드는 항상 고정 URL.
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("GGO_HALL_URL") {
        return url;
    }
    HALL_URL.to_string()
}

pub async fn fetch_hall() -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(hall_url())
        .send()
        .await
        .map_err(|e| format!("명예의 전당 다운로드 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("명예의 전당 HTTP {}", resp.status()));
    }
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("명예의 전당 JSON 파싱 실패: {e}"))
}
