import { useEffect, useState } from "react";
import { Modal } from "./Modal";
import { api } from "./api";
import type { FoundPlugin } from "./types";

type Props = {
  open: boolean;
  /** 이미 등록된 플러그인 경로 — 목록에서 "등록됨"으로 표시 */
  registered: string[];
  onClose: () => void;
  onAdd: (paths: string[]) => void;
};

const KIND_LABEL: Record<FoundPlugin["kind"], string> = {
  re: "RazorEnhanced",
  ca: "ClassicAssist",
  other: "기타 (ClassicUO 설정에 등록됨)",
};

const sameKey = (path: string) => path.replace(/\//g, "\\").toLowerCase();

/** PC에 설치된 RazorEnhanced / ClassicAssist를 찾아 골라서 등록하는 창. */
export function PluginDiscoverModal({ open, registered, onClose, onAdd }: Props) {
  const [found, setFound] = useState<FoundPlugin[] | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setFound(null);
    setError("");
    api
      .discoverInstallations()
      .then((result) => {
        if (cancelled) return;
        const known = new Set(registered.map(sameKey));
        // 종류별 첫 후보(아직 등록 안 된 것)만 미리 체크
        const preset = new Set<string>();
        const seenKind = new Set<string>();
        for (const p of result.plugins) {
          if (p.kind === "other" || known.has(sameKey(p.path)) || seenKind.has(p.kind)) continue;
          seenKind.add(p.kind);
          preset.add(p.path);
        }
        setFound(result.plugins);
        setChecked(preset);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
    // registered는 열 때 한 번만 반영
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const known = new Set(registered.map(sameKey));
  const toggle = (path: string) =>
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  return (
    <Modal open={open} onClose={onClose} title="플러그인 자동 찾기" width={620}>
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {error ? (
          <p style={{ margin: 0 }}>찾는 중 오류가 났습니다: {error}</p>
        ) : found === null ? (
          <p style={{ margin: 0, opacity: 0.8 }}>이 PC에서 플러그인을 찾는 중입니다…</p>
        ) : found.length === 0 ? (
          <p style={{ margin: 0, lineHeight: 1.5 }}>
            찾은 플러그인이 없습니다. 「+ 추가」로 RazorEnhanced.exe 또는
            ClassicAssist.dll을 직접 골라 주세요.
          </p>
        ) : (
          <>
            <p style={{ margin: 0, lineHeight: 1.5 }}>
              등록할 플러그인을 고르세요. 파일은 원래 위치 그대로 사용합니다.
            </p>
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                maxHeight: 320,
                overflowY: "auto",
              }}
            >
              {found.map((p) => {
                const already = known.has(sameKey(p.path));
                return (
                  <label
                    key={p.path}
                    className="modal-path-box"
                    style={{
                      display: "flex",
                      gap: 10,
                      alignItems: "flex-start",
                      cursor: already ? "default" : "pointer",
                      opacity: already ? 0.55 : 1,
                    }}
                  >
                    <input
                      type="checkbox"
                      disabled={already}
                      checked={already || checked.has(p.path)}
                      onChange={() => toggle(p.path)}
                      style={{ marginTop: 3 }}
                    />
                    <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
                      <span style={{ fontSize: 13 }}>
                        {KIND_LABEL[p.kind]}
                        {already && " · 등록됨"}
                      </span>
                      <span className="modal-path-value" style={{ wordBreak: "break-all" }}>
                        {p.path}
                      </span>
                    </div>
                  </label>
                );
              })}
            </div>
          </>
        )}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
          <button className="btn-action" onClick={onClose}>
            닫기
          </button>
          <button
            className="btn-action"
            disabled={checked.size === 0}
            onClick={() => {
              onAdd(found?.filter((p) => checked.has(p.path)).map((p) => p.path) ?? []);
              onClose();
            }}
          >
            {checked.size > 0 ? `${checked.size}개 등록` : "등록"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
