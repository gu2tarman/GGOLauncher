import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api } from "./api";
import { Modal } from "./Modal";
import type { Featured, Hall } from "./types";

/**
 * 사이드바 하단 피처 카드 (UO 챔피언 서바이버) + 명예의 전당 모달.
 * 카드 설정은 sidebar.json `featured`, 순위는 서바이버 공개 API(Rust 고정 URL).
 * 순위를 못 받아오면 순환 줄만 숨기고 카드·플레이는 그대로 동작.
 */

/** 번들 기본 그림. featured.image_url(HTTPS)이 로드되면 교체. */
const DEFAULT_IMAGE = "/survivor-card.jpg";
const ROTATE_MS = 2500;
const HALL_REFRESH_MS = 10 * 60 * 1000;

const JOB_NAME: Record<string, string> = {
  warrior: "전사",
  mage: "메이지",
  ranger: "레인저",
  ninja: "닌자",
};

function formatTime(sec: number): string {
  const s = Math.floor(sec);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function httpsOnly(value?: string | null): string | null {
  if (!value?.trim()) return null;
  try {
    const url = new URL(value.trim());
    return url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

/** 순환 한 칸 = 한 보드의 1위. 순서는 서버가 준 스테이지·보드 순서 그대로. */
type RotationItem = {
  stage: number;
  board: number;
  label: string;
  nickname: string;
  time: number;
};

function rotationOf(hall: Hall | null): RotationItem[] {
  if (!hall) return [];
  const items: RotationItem[] = [];
  hall.stages.forEach((st, si) =>
    st.boards.forEach((b, bi) => {
      const first = b.top[0];
      if (!first) return;
      items.push({
        stage: si,
        board: bi,
        label: b.job ? `${st.short} ${b.label}` : st.short,
        nickname: first.nickname,
        time: first.time,
      });
    })
  );
  return items;
}

function TrophyIcon() {
  return (
    <svg className="featured-trophy" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M8 21h8M12 17v4M7 4h10v5a5 5 0 0 1-10 0z" />
      <path d="M17 5h3v2a3 3 0 0 1-3 3M7 5H4v2a3 3 0 0 0 3 3" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <path d="M2 1l7 4-7 4z" fill="currentColor" />
    </svg>
  );
}

export function SurvivorCard({ featured }: { featured: Featured }) {
  const showHall = featured.hall === true;
  const [image, setImage] = useState(DEFAULT_IMAGE);
  const [hall, setHall] = useState<Hall | null>(null);
  const [hallError, setHallError] = useState<string | null>(null);
  const [hallNonce, setHallNonce] = useState(0);
  const [modal, setModal] = useState<{ stage: number; board: number } | null>(null);

  // 원격 그림은 프리로드 성공 시에만 교체 (배경 아트와 같은 방식)
  useEffect(() => {
    const remote = httpsOnly(featured.image_url);
    if (!remote) {
      setImage(DEFAULT_IMAGE);
      return;
    }
    let cancelled = false;
    const img = new Image();
    img.onload = () => {
      if (!cancelled) setImage(remote);
    };
    img.onerror = () => console.warn("[featured image] 로드 실패", remote);
    img.src = remote;
    return () => {
      cancelled = true;
    };
  }, [featured.image_url]);

  // 명예의 전당 데이터: 시작 시 + 10분마다 (+ 모달의 다시 시도)
  useEffect(() => {
    if (!showHall) return;
    let cancelled = false;
    const load = () =>
      api
        .fetchSurvivorHall()
        .then((h) => {
          if (cancelled) return;
          setHall(h);
          setHallError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          console.warn("[survivor hall]", e);
          setHallError(String(e));
        });
    load();
    const t = window.setInterval(load, HALL_REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [showHall, hallNonce]);

  const rotation = useMemo(() => rotationOf(hall), [hall]);

  // 순환: 멈춤 조건은 매 틱 검사(호버·모달·창 숨김). mouseenter/leave 플래그는
  // 모달이 줄을 덮으면 leave가 안 와서 멈춤이 고착된다.
  // 모달이 닫히면 effect가 다시 걸려 간격을 처음부터 센다.
  const rotRef = useRef<HTMLDivElement>(null);
  const [rot, setRot] = useState({ cur: 0, prev: -1, seq: 0 });
  const modalOpen = modal !== null;
  useEffect(() => {
    if (rotation.length < 2 || modalOpen) return;
    const t = window.setInterval(() => {
      if (document.visibilityState !== "visible") return;
      if (rotRef.current?.matches(":hover")) return;
      setRot((r) => ({ cur: (r.cur + 1) % rotation.length, prev: r.cur, seq: r.seq + 1 }));
    }, ROTATE_MS);
    return () => window.clearInterval(t);
  }, [rotation.length, modalOpen]);

  // 데이터 갱신으로 항목 수가 줄면 범위 안으로
  const cur = rotation.length > 0 ? rotation[rot.cur % rotation.length] : null;
  const prev = rot.prev >= 0 && rot.prev < rotation.length ? rotation[rot.prev] : null;

  const openPlay = () => api.openExternal(featured.url).catch(console.error);

  return (
    <div className="featured-card">
      <div className="featured-img" style={{ backgroundImage: `url("${image}")` }} aria-hidden />
      <div className="featured-shade" aria-hidden />
      <div className="featured-body">
        <div className="featured-title">{featured.title}</div>
        {featured.subtitle && <div className="featured-sub">{featured.subtitle}</div>}
        {showHall && cur && (
          <div className="featured-rot" ref={rotRef}>
            {prev && rot.seq > 0 && (
              <div key={`p${rot.seq}`} className="featured-rank featured-rank-out" aria-hidden>
                <RankLine item={prev} />
              </div>
            )}
            <button
              key={`c${rot.seq}`}
              className={`featured-rank ${rot.seq > 0 ? "featured-rank-in" : ""}`}
              onClick={() => setModal({ stage: cur.stage, board: cur.board })}
              title={`${cur.nickname} — 명예의 전당 열기`}
            >
              <RankLine item={cur} />
            </button>
          </div>
        )}
        <div className="featured-actions">
          <button className="featured-btn featured-btn-primary" onClick={openPlay} title="외부 브라우저로 열기">
            <PlayIcon /> 플레이
          </button>
          {showHall && (
            <button className="featured-btn" onClick={() => setModal({ stage: 0, board: 0 })} title="명예의 전당 열기">
              <TrophyIcon /> 명예의 전당
            </button>
          )}
        </div>
      </div>

      {/* 사이드바(backdrop-filter)가 fixed 요소의 기준이 되므로 모달은 body로 포털 */}
      {modal && createPortal(
        <HallOfFameModal
          hall={hall}
          error={hallError}
          image={image}
          initial={modal}
          onRetry={() => setHallNonce((n) => n + 1)}
          onOpenGame={openPlay}
          onClose={() => setModal(null)}
        />,
        document.body
      )}
    </div>
  );
}

function RankLine({ item }: { item: RotationItem }) {
  return (
    <>
      <TrophyIcon />
      <span className="featured-rank-label">{item.label}</span>
      <b className="featured-rank-name">{item.nickname}</b>
      <span className="featured-rank-time">{formatTime(item.time)} ›</span>
    </>
  );
}

type ModalProps = {
  hall: Hall | null;
  error: string | null;
  image: string;
  initial: { stage: number; board: number };
  onRetry: () => void;
  onOpenGame: () => void;
  onClose: () => void;
};

function HallOfFameModal({ hall, error, image, initial, onRetry, onOpenGame, onClose }: ModalProps) {
  const [stageIdx, setStageIdx] = useState(initial.stage);
  const [boardIdx, setBoardIdx] = useState(initial.board);

  const stage = hall?.stages[stageIdx] ?? hall?.stages[0] ?? null;
  const board = stage ? stage.boards[boardIdx] ?? stage.boards[0] : null;
  const perJob = !!stage && stage.boards.length > 1;

  const selectStage = (i: number) => {
    setStageIdx(i);
    setBoardIdx(0);
  };

  return (
    <Modal open onClose={onClose} title="명예의 전당 · UO 챔피언 서바이버" width={680}>
      <div className="hall-hero" style={{ backgroundImage: `url("${image}")` }}>
        <div className="hall-hero-text">
          <div className="hall-kicker">{hall?.banner.kicker ?? "UO 챔피언 서바이버"}</div>
          <div className="hall-headline">{hall?.banner.title ?? "직업별 1위는 런처에 이름이 남습니다"}</div>
        </div>
      </div>

      {!hall && error && (
        <div className="hall-empty">
          순위를 불러오지 못했습니다
          <button className="btn-action" onClick={onRetry}>다시 시도</button>
        </div>
      )}
      {!hall && !error && <div className="hall-empty">불러오는 중...</div>}

      {stage && board && (
        <>
          <div className="hall-tabs">
            {hall!.stages.map((st, i) => (
              <button key={st.id} className={`hall-tab ${st === stage ? "is-on" : ""}`} onClick={() => selectStage(i)}>
                {st.label}
              </button>
            ))}
          </div>
          {perJob && (
            <div className="hall-jobs">
              {stage.boards.map((b, i) => (
                <button key={b.label} className={`hall-job ${b === board ? "is-on" : ""}`} onClick={() => setBoardIdx(i)}>
                  {b.label}
                </button>
              ))}
            </div>
          )}
          <div className="hall-section">
            {perJob ? `${stage.label} · ${board.label}` : stage.label} 순위
          </div>
          {board.top.length === 0 ? (
            <div className="hall-empty">아직 클리어 기록이 없습니다 — 첫 이름을 남겨 보세요</div>
          ) : (
            board.top.map((e) => (
              <div key={`${e.rank}-${e.nickname}`} className={`hall-row hall-rank-${e.rank}`}>
                <div className="hall-rank">{e.rank}</div>
                <div className="hall-name">
                  <b>{e.nickname}</b>
                  {(e.title || !board.job) && (
                    <span>
                      {!board.job && (JOB_NAME[e.character] ?? e.character)}
                      {!board.job && e.title && " · "}
                      {e.title}
                    </span>
                  )}
                </div>
                <div className="hall-time">{formatTime(e.time)}</div>
              </div>
            ))
          )}
          {board.past && (
            <div className="hall-past">
              지난 시즌{board.job ? ` ${board.label}` : ""} 1위 · <b>{board.past.nickname}</b> {formatTime(board.past.time)}
            </div>
          )}
        </>
      )}

      <div className="hall-foot">
        <span>{hall ? `시즌 ${hall.season} · 클리어 기록만 · 상위 ${hall.top}명` : ""}</span>
        <button className="featured-btn featured-btn-primary hall-foot-btn" onClick={onOpenGame}>
          게임에서 전체 순위 보기
        </button>
      </div>
    </Modal>
  );
}
