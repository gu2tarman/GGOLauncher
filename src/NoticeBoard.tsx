import { useEffect, useMemo, useState } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { api } from "./api";
import type { Notice } from "./types";

// 마크다운 옵션: 줄바꿈 = <br>, GFM
marked.setOptions({ gfm: true, breaks: true });

// 외부 공지가 임의 HTML/스크립트를 주입 못 하게 sanitize.
// 링크는 별도 "전체 보기" 버튼으로 처리하므로 본문 <a>는 차단.
const SANITIZE_CONFIG = {
  ALLOWED_TAGS: [
    "p", "br", "strong", "em", "b", "i", "u", "del", "s", "code", "pre",
    "blockquote", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6",
    "hr", "span", "div",
  ],
  ALLOWED_ATTR: ["class"],
};

type Source = "margo" | "ggouo";

type Props = {
  source: Source;
  title: string;
};

export function NoticeBoard({ source, title }: Props) {
  const [items, setItems] = useState<Notice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [retryNonce, setRetryNonce] = useState(0);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setItems(null);
    setError(null);
    api
      .fetchNotice()
      .then((board) => {
        if (cancelled) return;
        const list = board[source] ?? [];
        setItems(list);
        setError(null);
        // 첫(최신) 공지 자동 펼침
        setExpandedId(list[0]?.id ?? null);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setItems(null);
      });
    return () => {
      cancelled = true;
    };
  }, [source, retryNonce]);

  const retry = () => setRetryNonce((n) => n + 1);

  return (
    <div className="notice-card">
      <header className="panel-header">📢 {title}</header>
      <div className="notice-list">
        {error && (
          <div className="notice-error" title={error}>
            불러오기 실패
            <div style={{ fontSize: 11, marginTop: 4, opacity: 0.85, fontFamily: "ui-monospace, Consolas, monospace", whiteSpace: "pre-wrap", textAlign: "left" }}>
              {error}
            </div>
            <button className="btn-small" style={{ marginTop: 8 }} onClick={retry}>
              재시도
            </button>
          </div>
        )}
        {!error && items === null && (
          <div className="notice-placeholder">불러오는 중...</div>
        )}
        {!error && items?.length === 0 && (
          <div className="notice-placeholder">공지가 없습니다</div>
        )}
        {items?.map((n) => (
          <NoticeItem
            key={n.id}
            notice={n}
            expanded={expandedId === n.id}
            onToggle={() =>
              setExpandedId((cur) => (cur === n.id ? null : n.id))
            }
          />
        ))}
      </div>
    </div>
  );
}

type ItemProps = {
  notice: Notice;
  expanded: boolean;
  onToggle: () => void;
};

function NoticeItem({ notice, expanded, onToggle }: ItemProps) {
  const html = useMemo(() => {
    const raw = marked.parse(notice.body_md) as string;
    return DOMPurify.sanitize(raw, SANITIZE_CONFIG);
  }, [notice.body_md]);

  return (
    <article className={`notice-item severity-${notice.severity} ${expanded ? "is-expanded" : ""}`}>
      <button
        type="button"
        className="notice-item-head"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="notice-badge">{labelOf(notice.severity)}</span>
        <span className="notice-title">{notice.title}</span>
        <span className="notice-date">{notice.date}</span>
        <span className={`notice-caret ${expanded ? "is-open" : ""}`}>▾</span>
      </button>
      {expanded && (
        <>
          <div
            className="notice-body"
            dangerouslySetInnerHTML={{ __html: html }}
          />
          {notice.url && (
            <div className="notice-more">
              <button
                type="button"
                className="btn-notice-more"
                onClick={(e) => {
                  e.stopPropagation();
                  api.openExternal(notice.url!).catch(console.error);
                }}
              >
                전체 보기 →
              </button>
            </div>
          )}
        </>
      )}
    </article>
  );
}

function labelOf(s: Notice["severity"]): string {
  switch (s) {
    case "urgent": return "긴급";
    case "event": return "이벤트";
    default: return "공지";
  }
}
