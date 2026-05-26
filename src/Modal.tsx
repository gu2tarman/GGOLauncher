import { useEffect, type ReactNode } from "react";

type Props = {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  width?: number;
  /** 헤더 우측, 닫기 버튼 앞에 끼워넣을 액션 (선택). */
  headerActions?: ReactNode;
  children: ReactNode;
};

// ── 모달 ESC 스택 ───────────────────────────────────────────
// 모달 여러 개가 동시에 열려있을 때 ESC가 모든 리스너를 발화시키지 않고
// "최상단(가장 늦게 열린)" 모달의 onClose만 호출하도록 LIFO 스택으로 관리.
const escStack: Array<() => void> = [];
let escListenerInstalled = false;

function ensureEscListener() {
  if (escListenerInstalled) return;
  window.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || escStack.length === 0) return;
    const top = escStack[escStack.length - 1];
    top();
  });
  escListenerInstalled = true;
}

function pushEsc(onClose: () => void): () => void {
  ensureEscListener();
  escStack.push(onClose);
  return () => {
    const i = escStack.lastIndexOf(onClose);
    if (i >= 0) escStack.splice(i, 1);
  };
}

/**
 * 공용 모달. ESC 또는 X 버튼으로만 닫힘.
 * (백드롭 클릭으로는 닫지 않음 — 폼 작성 중 실수로 변경사항 날리는 것 방지)
 * ESC는 LIFO 스택 기반 — 가장 위에 떠 있는 모달이 먼저 닫힘.
 */
export function Modal({
  open,
  onClose,
  title,
  width = 720,
  headerActions,
  children,
}: Props) {
  useEffect(() => {
    if (!open) return;
    return pushEsc(onClose);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="modal-backdrop">
      <div
        className="modal-dialog"
        style={{ width }}
      >
        <header className="modal-head">
          <h2 className="modal-title">{title}</h2>
          <div className="modal-head-actions">
            {headerActions}
            <button className="modal-close" onClick={onClose} aria-label="닫기">
              ×
            </button>
          </div>
        </header>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  );
}
