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

  let dotColor = "#999";
  let stateText = "확인 중";
  let stateColor = "rgba(255,255,255,0.85)";
  if (display.kind === "online") {
    dotColor = "#5ad17e";
    stateText = "온라인";
    stateColor = "#5ad17e";
  } else if (display.kind === "offline") {
    dotColor = "#e26a6a";
    stateText = "오프라인";
    stateColor = "#e26a6a";
  }

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
      role="status"
      aria-label={`${endpoint.label || "서버"} ${stateText}${msText ? ` ${msText}` : ""}`}
      title={detailText}
      style={{
        background: "rgba(255,255,255,0.04)",
        border: `1px solid ${dotColor}55`,
        borderRadius: 8,
        padding: "6px 10px",
        display: "flex",
        alignItems: "center",
        gap: 8,
        fontSize: 12,
        width: "calc(100% - 32px)",
        boxSizing: "border-box",
      }}
    >
      <span
        aria-hidden
        style={{
          width: 9,
          height: 9,
          borderRadius: "50%",
          background: dotColor,
          boxShadow:
            display.kind === "online" ? `0 0 6px ${dotColor}aa` : "none",
          flexShrink: 0,
          transition: "background 0.2s, box-shadow 0.2s",
        }}
      />
      <span style={{ fontWeight: 600 }}>{endpoint.label || "마고 서버"}</span>
      <span style={{ color: stateColor, fontWeight: 600 }}>{stateText}</span>
      {msText && (
        <span
          style={{
            marginLeft: "auto",
            opacity: 0.75,
            fontFamily: "ui-monospace, Consolas, monospace",
            fontSize: 11,
          }}
        >
          {msText}
        </span>
      )}
    </div>
  );
}
