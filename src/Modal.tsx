import { useEffect, type ReactNode } from "react";

type Props = {
  open: boolean;
  onClose: () => void;
  title: string;
  width?: number;
  /** 헤더 우측, 닫기 버튼 앞에 끼워넣을 액션 (선택). */
  headerActions?: ReactNode;
  children: ReactNode;
};

/**
 * 공용 모달. 백드롭 클릭 + ESC = 닫기.
 * Tauri 환경이라 portal 없이도 충돌 없음.
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
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal-dialog"
        style={{ width }}
        onClick={(e) => e.stopPropagation()}
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
