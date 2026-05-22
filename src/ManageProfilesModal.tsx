import { useMemo, useState } from "react";
import { Modal } from "./Modal";
import type { Profile, Settings } from "./types";

type Props = {
  open: boolean;
  settings: Settings;
  onClose: () => void;
  onChange: (next: Settings) => void;
  onEdit: (profileId: string) => void;
};

export function ManageProfilesModal({
  open,
  settings,
  onClose,
  onChange,
  onEdit,
}: Props) {
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  // 서버 호스트별 그룹화
  const grouped = useMemo(() => {
    const map = new Map<string, Profile[]>();
    for (const p of settings.profiles) {
      const key = `${p.server.address}:${p.server.port}`;
      const arr = map.get(key) ?? [];
      arr.push(p);
      map.set(key, arr);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [settings.profiles]);

  const onCreate = () => {
    const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
    const newProfile: Profile = {
      id,
      name: `New Profile ${settings.profiles.length + 1}`,
      uo_path: "",
      cuo_path: null,
      client_version: null,
      server: {
        address: "login.uoserver.com",
        port: 2593,
        username: "",
        password_encrypted: "",
        encryption: "auto",
      },
    };
    onChange({
      ...settings,
      profiles: [...settings.profiles, newProfile],
      active_profile_id: settings.active_profile_id ?? id,
    });
    onEdit(id);
  };

  const onSetActive = (id: string) => {
    onChange({ ...settings, active_profile_id: id });
  };

  const onCopy = (p: Profile) => {
    const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
    const copy: Profile = { ...p, id, name: `${p.name} (복사)` };
    onChange({ ...settings, profiles: [...settings.profiles, copy] });
  };

  const onDeleteConfirmed = (id: string) => {
    const profiles = settings.profiles.filter((p) => p.id !== id);
    const active =
      settings.active_profile_id === id
        ? profiles[0]?.id ?? null
        : settings.active_profile_id;
    onChange({ ...settings, profiles, active_profile_id: active });
    setConfirmDelete(null);
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="프로필 관리"
      width={780}
      headerActions={
        <button className="btn-primary btn-primary-sm" onClick={onCreate}>
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
        {grouped.map(([host, list]) => (
          <section key={host} className="profile-group">
            <header className="profile-group-head">{host}</header>
            <div className="profile-cards">
              {list.map((p) => {
                const isActive = settings.active_profile_id === p.id;
                return (
                  <div
                    key={p.id}
                    className={`profile-card ${isActive ? "is-active" : ""}`}
                    role="button"
                    tabIndex={0}
                    title={isActive ? "현재 활성 프로필" : "클릭하여 활성화"}
                    onClick={() => !isActive && onSetActive(p.id)}
                    onKeyDown={(e) => {
                      if ((e.key === "Enter" || e.key === " ") && !isActive) {
                        e.preventDefault();
                        onSetActive(p.id);
                      }
                    }}
                  >
                    <div className="profile-card-main">
                      <div className="profile-card-title">
                        {p.name}
                        {isActive && <span className="active-badge">ACTIVE</span>}
                      </div>
                      <div className="profile-card-meta">
                        <span className="meta-label">계정</span>
                        <span>{p.server.username || "—"}</span>
                        <span className="meta-sep">·</span>
                        <span className="meta-label">포트</span>
                        <span>{p.server.port}</span>
                      </div>
                    </div>
                    <div
                      className="profile-card-actions"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <button className="btn-action" onClick={() => onEdit(p.id)}>
                        Edit
                      </button>
                      <button className="btn-action" onClick={() => onCopy(p)}>
                        Copy
                      </button>
                      <button
                        className="btn-action btn-action-danger"
                        onClick={() => setConfirmDelete(p.id)}
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        ))}
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
