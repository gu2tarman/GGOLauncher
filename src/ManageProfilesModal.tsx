import { useRef, useState } from "react";
import { Modal } from "./Modal";
import type { Profile, Settings } from "./types";

type Props = {
  open: boolean;
  settings: Settings;
  onClose: () => void;
  onChange: (next: Settings) => void;
  /** 기존 프로필 편집 */
  onEdit: (profileId: string) => void;
  /** 새 프로필 드래프트 편집 시작 (저장 누를 때까지 settings 미반영) */
  onCreate: (draft: Profile) => void;
};

export function ManageProfilesModal({
  open,
  settings,
  onClose,
  onChange,
  onEdit,
  onCreate,
}: Props) {
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [previewOrder, setPreviewOrder] = useState<string[] | null>(null);
  const pointerDragRef = useRef<{
    id: string;
    startX: number;
    startY: number;
    active: boolean;
  } | null>(null);
  const previewOrderRef = useRef<string[] | null>(null);

  const displayProfiles = (previewOrder ?? settings.profiles.map((p) => p.id))
    .map((id) => settings.profiles.find((profile) => profile.id === id))
    .filter((profile): profile is Profile => !!profile);

  const updatePreviewOrder = (next: string[] | null) => {
    previewOrderRef.current = next;
    setPreviewOrder(next);
  };

  const clearDragState = () => {
    pointerDragRef.current = null;
    setDraggedId(null);
    updatePreviewOrder(null);
  };

  const reorderIds = (
    order: string[],
    id: string,
    targetId: string,
    afterTarget: boolean
  ) => {
    if (id === targetId) return order;
    const next = [...order];
    const from = next.indexOf(id);
    const target = next.indexOf(targetId);
    if (from < 0 || target < 0) return order;
    const [moved] = next.splice(from, 1);
    const targetAfterRemoval = next.indexOf(targetId);
    next.splice(targetAfterRemoval + (afterTarget ? 1 : 0), 0, moved);
    return next;
  };

  const onCreateClick = () => {
    const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
    const draft: Profile = {
      id,
      name: `새 프로필 ${settings.profiles.length + 1}`,
      uo_path: "",
      cuo_path: null,
      client_version: null,
      server: {
        address: "login.uoserver.com",
        port: 2593,
        encryption: "auto",
        accounts: [],
        active_account_id: null,
      },
    };
    // settings 변경 안 함 — EditProfileModal에서 저장 누를 때 부모가 upsert.
    onCreate(draft);
  };

  const onCopy = (p: Profile) => {
    const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
    const copy: Profile = { ...p, id, name: `${p.name} (복사)` };
    onChange({ ...settings, profiles: [...settings.profiles, copy] });
  };

  const onDeleteConfirmed = (id: string) => {
    const removedIndex = settings.profiles.findIndex((p) => p.id === id);
    const profiles = settings.profiles.filter((p) => p.id !== id);
    const active =
      settings.active_profile_id === id
        ? profiles[Math.min(Math.max(removedIndex, 0), profiles.length - 1)]?.id ?? null
        : settings.active_profile_id;
    onChange({
      ...settings,
      profiles,
      active_profile_id: active,
    });
    setConfirmDelete(null);
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={
        <span className="modal-title-with-hint">
          프로필 관리
          <span className="modal-title-hint">드래그하여 메인 노출 순서 변경</span>
        </span>
      }
      width={780}
      headerActions={
        <button className="btn-primary btn-primary-sm" onClick={onCreateClick}>
          + 새 프로필
        </button>
      }
    >
      {settings.profiles.length === 0 && (
        <div className="empty-state">
          <div className="empty-title">등록된 프로필이 없습니다</div>
          <div className="empty-hint">"+ 새 프로필"로 첫 프로필을 만드세요</div>
        </div>
      )}

      <div className="profile-groups">
          <section className="profile-group">
            <div className="profile-cards">
              {displayProfiles.map((p, index) => {
                const isCurrent = settings.active_profile_id === p.id;
                return (
                  <div
                    key={p.id}
                    className={`profile-card ${isCurrent ? "is-current" : ""} ${
                      draggedId === p.id ? "is-dragging" : ""
                    }`}
                    data-profile-id={p.id}
                    onPointerDown={(e) => {
                      if (
                        e.target instanceof Element &&
                        e.target.closest("button")
                      ) {
                        return;
                      }
                      if (e.button !== 0) return;
                      e.currentTarget.setPointerCapture(e.pointerId);
                      pointerDragRef.current = {
                        id: p.id,
                        startX: e.clientX,
                        startY: e.clientY,
                        active: false,
                      };
                      updatePreviewOrder(settings.profiles.map((profile) => profile.id));
                    }}
                    onPointerMove={(e) => {
                      const drag = pointerDragRef.current;
                      if (!drag) return;
                      if (
                        !drag.active &&
                        Math.hypot(
                          e.clientX - drag.startX,
                          e.clientY - drag.startY
                        ) < 5
                      ) {
                        return;
                      }
                      if (!drag.active) {
                        drag.active = true;
                        setDraggedId(drag.id);
                      }
                      e.preventDefault();
                      const target = document
                        .elementFromPoint(e.clientX, e.clientY)
                        ?.closest<HTMLElement>("[data-profile-id]");
                      const targetId = target?.dataset.profileId;
                      if (!target || !targetId || targetId === drag.id) {
                        return;
                      }
                      const current = previewOrderRef.current;
                      if (current) {
                        const sourceIndex = current.indexOf(drag.id);
                        const targetIndex = current.indexOf(targetId);
                        // 목표 카드 영역에 들어오는 즉시 이동 방향대로 앞/뒤에 배치.
                        const after = sourceIndex < targetIndex;
                        const next = reorderIds(current, drag.id, targetId, after);
                        if (next.some((id, index) => id !== current[index])) {
                          updatePreviewOrder(next);
                        }
                      }
                    }}
                    onPointerUp={(e) => {
                      const drag = pointerDragRef.current;
                      const order = previewOrderRef.current;
                      if (drag?.active && order) {
                        const profiles = order
                          .map((id) =>
                            settings.profiles.find((profile) => profile.id === id)
                          )
                          .filter((profile): profile is Profile => !!profile);
                        if (
                          profiles.some(
                            (profile, index) =>
                              profile.id !== settings.profiles[index]?.id
                          )
                        ) {
                          onChange({ ...settings, profiles });
                        }
                      }
                      if (e.currentTarget.hasPointerCapture(e.pointerId)) {
                        e.currentTarget.releasePointerCapture(e.pointerId);
                      }
                      clearDragState();
                    }}
                    onPointerCancel={clearDragState}
                  >
                    <span className="profile-order-number">{index + 1}</span>
                    <div className="profile-card-main">
                      <div className="profile-card-title">
                        {p.name}
                        {isCurrent && <span className="current-badge">현재 사용 중</span>}
                      </div>
                      <div className="profile-card-meta">
                        <span className="meta-label">서버</span>
                        <span>{p.server.address}:{p.server.port}</span>
                      </div>
                    </div>
                    <div className="profile-card-actions">
                      <button
                        className="btn-action"
                        onClick={() => onEdit(p.id)}
                        title="이 프로필 편집 (서버/경로/계정 수정)"
                      >
                        편집
                      </button>
                      <button
                        className="btn-action"
                        onClick={() => onCopy(p)}
                        title="같은 설정으로 복제본 만들기"
                      >
                        복사
                      </button>
                      <button
                        className="btn-action btn-action-danger"
                        onClick={() => setConfirmDelete(p.id)}
                        title="이 프로필 삭제 (되돌릴 수 없음)"
                      >
                        삭제
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
      </div>

      {/* 삭제 확인 다이얼로그 */}
      {confirmDelete && (
        <div className="confirm-overlay" onClick={() => setConfirmDelete(null)}>
          <div className="confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="confirm-title">프로필 삭제</div>
            <div className="confirm-body">
              "{settings.profiles.find((p) => p.id === confirmDelete)?.name}" 프로필을
              삭제할까요? 되돌릴 수 없습니다.
            </div>
            <div className="confirm-actions">
              <button className="btn-action" onClick={() => setConfirmDelete(null)}>
                취소
              </button>
              <button
                className="btn-action btn-action-danger"
                onClick={() => onDeleteConfirmed(confirmDelete)}
              >
                삭제
              </button>
            </div>
          </div>
        </div>
      )}
    </Modal>
  );
}
