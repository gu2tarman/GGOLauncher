import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { api } from "./api";
import type { Notice } from "./types";

// 마크다운 옵션: 줄바꿈 = <br>, GFM
marked.setOptions({ gfm: true, breaks: true });

// 외부 공지가 임의 HTML/스크립트를 주입 못 하게 sanitize.
// 본문 링크는 http/https만 허용하고 클릭 시 Tauri open_external 검증을 다시 거친다.
const SANITIZE_CONFIG = {
  ALLOWED_TAGS: [
    "p", "br", "strong", "em", "b", "i", "u", "del", "s", "code", "pre", "a",
    "blockquote", "ul", "ol", "li", "h1", "h2", "h3", "h4", "h5", "h6",
    "hr", "span", "div",
  ],
  ALLOWED_ATTR: ["class", "href", "title"],
  ALLOWED_URI_REGEXP: /^https?:\/\//i,
};

const URL_PATTERN = /https?:\/\/[^\s<>"`]+/gi;

type Props = {
  title: string;
  items: Notice[] | null;
  loading: boolean;
  error: string | null;
  onRetry: () => void;
};

export function NoticeBoard({ title, items, loading, error, onRetry }: Props) {
  const [expandedId, setExpandedId] = useState<string | null>(null);

  useEffect(() => {
    setExpandedId((cur) =>
      cur && items?.some((item) => item.id === cur) ? cur : null
    );
  }, [items]);

  return (
    <div className="notice-card">
      <header className="panel-header">{title}</header>
      <div className="notice-list">
        {error && (
          <div className="notice-error" title={error}>
            불러오기 실패
            <div style={{ fontSize: 11, marginTop: 4, opacity: 0.85, fontFamily: "ui-monospace, Consolas, monospace", whiteSpace: "pre-wrap", textAlign: "left" }}>
              {error}
            </div>
            <button className="btn-small" style={{ marginTop: 8 }} onClick={onRetry}>
              재시도
            </button>
          </div>
        )}
        {!error && loading && (
          <div className="notice-placeholder">불러오는 중...</div>
        )}
        {!error && items?.length === 0 && (
          <div className="notice-placeholder">공지가 없습니다</div>
        )}
        {items?.map((n, index) => (
          <NoticeItem
            key={n.id}
            notice={n}
            isLatest={index === 0}
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
  isLatest: boolean;
  expanded: boolean;
  onToggle: () => void;
};

function NoticeItem({ notice, isLatest, expanded, onToggle }: ItemProps) {
  const html = useMemo(() => {
    const raw = marked.parse(linkifyInlineCodeUrls(notice.body_md)) as string;
    return DOMPurify.sanitize(raw, SANITIZE_CONFIG);
  }, [notice.body_md]);

  const actionLinks = useMemo(() => buildActionLinks(notice), [notice]);
  const displaySeverity = severityOf(notice, isLatest);

  const openNoticeUrl = (url: string) => {
    api.openExternal(url).catch(console.error);
  };

  const handleBodyClick = (e: MouseEvent<HTMLDivElement>) => {
    if (!(e.target instanceof Element)) return;
    const anchor = e.target.closest("a");
    if (!anchor) return;

    e.preventDefault();
    e.stopPropagation();
    const href = anchor.getAttribute("href");
    if (href) openNoticeUrl(href);
  };

  return (
    <article className={`notice-item severity-${displaySeverity} ${expanded ? "is-expanded" : ""}`}>
      <button
        type="button"
        className="notice-item-head"
        onClick={onToggle}
        aria-expanded={expanded}
        title={notice.title}
      >
        <span className="notice-badge">{labelOf(displaySeverity)}</span>
        <span className="notice-head-main">
          <span className="notice-title">{notice.title}</span>
          <span className="notice-date">{notice.date}</span>
        </span>
        <span className={`notice-caret ${expanded ? "is-open" : ""}`}>›</span>
      </button>
      {expanded && (
        <>
          <div
            className="notice-body"
            onClick={handleBodyClick}
            dangerouslySetInnerHTML={{ __html: html }}
          />
          {actionLinks.length > 0 && (
            <div className="notice-more">
              {actionLinks.map((link) => (
                <button
                  key={link.url}
                  type="button"
                  className="btn-notice-more"
                  onClick={(e) => {
                    e.stopPropagation();
                    openNoticeUrl(link.url);
                  }}
                >
                  {link.label} →
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </article>
  );
}

function severityOf(notice: Notice, isLatest: boolean): Notice["severity"] {
  if (notice.severity === "urgent") return "urgent";
  return isLatest ? "event" : "normal";
}

function labelOf(s: Notice["severity"]): string {
  switch (s) {
    case "urgent": return "긴급";
    case "event": return "새소식";
    default: return "공지";
  }
}

function buildActionLinks(notice: Notice): Array<{ url: string; label: string }> {
  const links = new Map<string, string>();
  if (notice.url && isHttpUrl(notice.url) && !bodyContainsUrl(notice.body_md, notice.url)) {
    links.set(notice.url, notice.url_label || "전체 보기");
  }

  return [...links].map(([url, label]) => ({ url, label }));
}

function isHttpUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

function linkifyInlineCodeUrls(markdown: string): string {
  return markdown.replace(/`(https?:\/\/[^`\s]+)`/gi, "[$1]($1)");
}

function bodyContainsUrl(body: string, url: string): boolean {
  for (const match of body.matchAll(URL_PATTERN)) {
    if (trimTrailingPunctuation(match[0]) === url) return true;
  }
  return false;
}

function trimTrailingPunctuation(url: string): string {
  return url.replace(/[),.;\]]+$/g, "");
}
