import { useState } from "react";
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
  const mainProfileIds =
    settings.ui.main_profile_ids ?? settings.profiles.slice(0, 2).map((p) => p.id);

  const onCreateClick = () => {
    const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
    const draft: Profile = {
      id,
      name: `New Profile ${settings.profiles.length + 1}`,
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
    const profiles = settings.profiles.filter((p) => p.id !== id);
    const main_profile_ids = mainProfileIds.filter((profileId) => profileId !== id);
    const active =
      settings.active_profile_id === id
        ? main_profile_ids[0] ?? null
        : settings.active_profile_id;
    onChange({
      ...settings,
      profiles,
      active_profile_id: active,
      ui: { ...settings.ui, main_profile_ids },
    });
    setConfirmDelete(null);
  };

  const onToggleMainProfile = (id: string) => {
    if (mainProfileIds.includes(id)) {
      const main_profile_ids = mainProfileIds.filter((profileId) => profileId !== id);
      onChange({
        ...settings,
        active_profile_id:
          settings.active_profile_id === id
            ? main_profile_ids[0] ?? null
            : settings.active_profile_id,
        ui: { ...settings.ui, main_profile_ids },
      });
      return;
    }

    if (mainProfileIds.length >= 2) return;

    const main_profile_ids = [...mainProfileIds, id];
    const active_profile_id =
      settings.active_profile_id && mainProfileIds.includes(settings.active_profile_id)
        ? settings.active_profile_id
        : id;

    onChange({
      ...settings,
      active_profile_id,
      ui: { ...settings.ui, main_profile_ids },
    });
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={
        <span className="modal-title-with-hint">
          프로필 관리
          <span className="modal-title-hint">메인 표시 프로필은 최대 2개까지 선택</span>
        </span>
      }
      width={780}
      headerActions={
        <button className="btn-primary btn-primary-sm" onClick={onCreateClick}>
          + New Profile
        </button>
      }
    >
      {settings.profiles.length === 0 && (
        <div className="empty-state">
          <div className="empty-title">등록된 프로필이 없습니다</div>
          <div className="empty-hint">"+ New Profile"로 첫 프로필을 만드세요</div>
        </div>
      )}

      <div className="profile-groups">
          <section className="profile-group">
            <div className="profile-cards">
              {settings.profiles.map((p) => {
                const isMain = mainProfileIds.includes(p.id);
                const isCurrent = settings.active_profile_id === p.id;
                const activeSlot = isMain ? mainProfileIds.indexOf(p.id) + 1 : null;
                return (
                  <div
                    key={p.id}
                    className={`profile-card ${isMain ? "is-active" : ""} ${
                      isCurrent ? "is-current" : ""
                    }`}
                    role="button"
                    tabIndex={0}
                    title={
                      isMain
                        ? `메인 표시 프로필 ${activeSlot}에서 제외`
                        : mainProfileIds.length >= 2
                        ? "메인 표시 프로필은 최대 2개까지 선택할 수 있습니다"
                        : "클릭하여 메인 표시 프로필로 선택"
                    }
                    onClick={() => onToggleMainProfile(p.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        onToggleMainProfile(p.id);
                      }
                    }}
                  >
                    <div className="profile-card-main">
                      <div className="profile-card-title">
                        {p.name}
                        {activeSlot && (
                          <span className="active-badge">ACTIVE {activeSlot}</span>
                        )}
                        {isCurrent && <span className="current-badge">SELECTED</span>}
                      </div>
                      <div className="profile-card-meta">
                        <span className="meta-label">서버</span>
                        <span>{p.server.address}:{p.server.port}</span>
                      </div>
                    </div>
                    <div
                      className="profile-card-actions"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <button
                        className="btn-action"
                        onClick={() => onEdit(p.id)}
                        title="이 프로필 편집 (서버/경로/계정 수정)"
                      >
                        Edit
                      </button>
                      <button
                        className="btn-action"
                        onClick={() => onCopy(p)}
                        title="같은 설정으로 복제본 만들기"
                      >
                        Copy
                      </button>
                      <button
                        className="btn-action btn-action-danger"
                        onClick={() => setConfirmDelete(p.id)}
                        title="이 프로필 삭제 (되돌릴 수 없음)"
                      >
                        Delete
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
