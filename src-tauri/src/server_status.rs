//! 마고 서버 가용성 체크.
//!
//! 엔드포인트(host:port + 라벨)는 ggoce-deploy/server-status.json에서 받아옴.
//! 차후 IP/포트 변경 시 런처 재배포 없이 JSON만 갱신.
//! TCP connect 성공 여부로 Online/Offline 판정 (응답시간 ms 같이 반환).

use serde::{Deserialize, Serialize};
use std::time::Duration;

const ENDPOINT_URL: &str =
    "https://raw.githubusercontent.com/gu2tarman/ggoce-deploy/main/server-status.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEndpoint {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerStatus {
    Online { latency_ms: u64 },
    Offline { reason: String },
}

/// ggoce-deploy 원격 JSON에서 엔드포인트 받아옴. 실패 시 Err — 프론트 fallback.
pub async fn fetch_endpoint() -> Result<ServerEndpoint, String> {
    let client = reqwest::Client::builder()
        .user_agent("GGOLauncher/0.1")
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let resp = client
        .get(ENDPOINT_URL)
        .send()
        .await
        .map_err(|e| format!("endpoint fetch 실패: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("endpoint HTTP {}", resp.status()));
    }
    let endpoint = resp
        .json::<ServerEndpoint>()
        .await
        .map_err(|e| format!("endpoint JSON 파싱 실패: {e}"))?;
    if endpoint.host.trim().is_empty() || endpoint.port == 0 {
        return Err("endpoint host/port가 비어 있습니다".into());
    }
    Ok(endpoint)
}

/// TCP connect 테스트. host:port 도달 가능하면 Online + 응답시간.
/// 타임아웃 또는 거부 시 Offline.
pub async fn check_status(host: &str, port: u16, timeout_ms: u64) -> ServerStatus {
    use std::time::Instant;
    use tokio::net::lookup_host;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let host = host.trim();
    if host.is_empty() || port == 0 {
        return ServerStatus::Offline {
            reason: "주소가 비어 있습니다".into(),
        };
    }

    let timeout_ms = timeout_ms.clamp(500, 10_000);
    let addrs: Vec<_> = match lookup_host((host, port)).await {
        Ok(it) => it.collect(),
        Err(e) => {
            return ServerStatus::Offline {
                reason: format!("DNS 실패: {e}"),
            };
        }
    };
    if addrs.is_empty() {
        return ServerStatus::Offline {
            reason: "주소 해석 실패".into(),
        };
    }

    let started = Instant::now();
    let dur = Duration::from_millis(timeout_ms);
    let connect_result = timeout(dur, async {
        let mut last_error = None;
        for addr in addrs {
            match TcpStream::connect(&addr).await {
                Ok(stream) => return Ok(stream),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error)
    })
    .await;

    match connect_result {
        Ok(Ok(_stream)) => {
            let latency_ms = started.elapsed().as_millis() as u64;
            ServerStatus::Online { latency_ms }
        }
        Ok(Err(Some(e))) => ServerStatus::Offline {
            reason: format!("연결 실패: {e}"),
        },
        Ok(Err(None)) => ServerStatus::Offline {
            reason: "연결 가능한 주소가 없습니다".into(),
        },
        Err(_) => ServerStatus::Offline {
            reason: format!("타임아웃 ({}ms)", timeout_ms),
        },
    }
}
