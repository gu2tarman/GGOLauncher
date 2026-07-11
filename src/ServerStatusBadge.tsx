import { useEffect, useState } from "react";
import { api } from "./api";
import type { ServerEndpoint, ServerStatus } from "./types";

const FALLBACK_ENDPOINT: ServerEndpoint = {
  host: "222.102.202.108",
  port: 2594,
  label: "마고 서버",
};

const POLL_INTERVAL_MS = 60_000;

type DisplayState =
  | { kind: "checking" }
  | { kind: "online"; ms: number }
  | { kind: "offline"; reason: string };

export function ServerStatusBadge() {
  const [endpoint, setEndpoint] = useState<ServerEndpoint>(FALLBACK_ENDPOINT);
  const [display, setDisplay] = useState<DisplayState>({ kind: "checking" });

  useEffect(() => {
    let cancelled = false;
    api
      .fetchServerEndpoint()
      .then((e) => {
        if (!cancelled && e?.host && e.port) setEndpoint(e);
      })
      .catch((e) => console.warn("[server-status endpoint]", e));
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const run = async () => {
      setDisplay((cur) => (cur.kind === "online" ? cur : { kind: "checking" }));
      try {
        const s: ServerStatus = await api.checkServerStatus(
          endpoint.host,
          endpoint.port,
          3000
        );
        if (cancelled) return;
        if (s.state === "online") {
          setDisplay({ kind: "online", ms: s.latency_ms });
        } else {
          setDisplay({ kind: "offline", reason: s.reason });
        }
      } catch (e) {
        if (!cancelled) {
          console.warn("[server-status check]", e);
          setDisplay({ kind: "offline", reason: String(e) });
        }
      }
    };
    run();
    const id = setInterval(run, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [endpoint.host, endpoint.port]);

  const stateText =
    display.kind === "online"
      ? "온라인"
      : display.kind === "offline"
        ? "오프라인"
        : "확인 중";

  const msText = display.kind === "online" ? `${display.ms}ms` : "";
  const detailText =
    display.kind === "online"
      ? `${endpoint.host}:${endpoint.port} · ${msText}`
      : display.kind === "offline"
        ? `${endpoint.host}:${endpoint.port} · ${display.reason}`
        : `${endpoint.host}:${endpoint.port} · 확인 중`;

  return (
    <div
      className="server-status-badge"
      data-state={display.kind}
      role="status"
      aria-label={`${endpoint.label || "서버"} ${stateText}${msText ? ` ${msText}` : ""}`}
      title={detailText}
    >
      <span className="status-dot" aria-hidden />
      <span className="status-label">{endpoint.label || "마고 서버"}</span>
      <span className="status-state">{stateText}</span>
      {msText && <span className="status-ms">{msText}</span>}
    </div>
  );
}
