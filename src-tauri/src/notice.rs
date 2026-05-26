use serde::{Deserialize, Serialize};

/// 공지사항 전체 구조 (단일 JSON에 두 섹션).
/// URL: https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/notice.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NoticeBoard {
    #[serde(default)]
    pub margo: Vec<Notice>,
    #[serde(default)]
    pub ggouo: Vec<Notice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notice {
    pub id: String,
    pub title: String,
    pub date: String,
    #[serde(default)]
    pub severity: Severity,
    pub body_md: String,
    /// 선택: 전체 본문 외부 링크 (GitHub release/markdown 등).
    /// 있으면 본문 끝에 외부 링크 버튼 노출.
    #[serde(default)]
    pub url: Option<String>,
    /// 선택: url 버튼에 표시할 짧은 라벨.
    #[serde(default)]
    pub url_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Normal,
    Urgent,
    Event,
}

const NOTICE_URL: &str =
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/notice.json";

pub async fn fetch() -> Result<NoticeBoard, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client 생성 실패: {e}"))?;

    let resp = client
        .get(NOTICE_URL)
        .send()
        .await
        .map_err(|e| format!("공지 다운로드 실패: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("공지 HTTP {}", resp.status()));
    }

    let board: NoticeBoard = resp
        .json()
        .await
        .map_err(|e| format!("공지 JSON 파싱 실패: {e}"))?;

    Ok(board)
}
