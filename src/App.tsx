import { useEffect, useState } from "react";
import { api } from "./api";
import { NoticeBoard } from "./NoticeBoard";
import { ManageProfilesModal } from "./ManageProfilesModal";
import { EditProfileModal } from "./EditProfileModal";
import { PluginPanel } from "./PluginPanel";
import type { PluginEntry, Profile, Settings } from "./types";

type LinkButtonProps = { label: string; url?: string };

function LinkButton({ label, url }: LinkButtonProps) {
  const disabled = !url;
  const onClick = () => {
    if (!url) return;
    api.openExternal(url).catch(console.error);
  };
  return (
    <button
      className={`side-btn ${disabled ? "side-btn-disabled" : ""}`}
      onClick={onClick}
      disabled={disabled}
      title={disabled ? "준비 중" : url}
    >
      {label}
      {disabled && <span className="badge-soon">준비중</span>}
    </button>
  );
}

function activeProfile(settings: Settings | null): Profile | null {
  if (!settings || !settings.active_profile_id) return null;
  return settings.profiles.find((p) => p.id === settings.active_profile_id) ?? null;
}

type UpdateState =
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; percent: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: "checking" });
  const [manageOpen, setManageOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [ggoceVersion, setGgoceVersion] = useState<string | null>(null);

  useEffect(() => {
    api
      .launcherInit()
      .then(setSettings)
      .catch((e) => setLoadError(String(e)));

    const t = setTimeout(() => setUpdateState({ kind: "uptodate" }), 1500);
    return () => clearTimeout(t);
  }, []);

  /** settings를 갱신하고 디스크에도 저장. */
  const persistSettings = (next: Settings) => {
    setSettings(next);
    api.saveSettings(next).catch((e) => console.error("save failed:", e));
  };

  const profile = activeProfile(settings);
  const hasProfile = !!profile;

  // 활성 프로필의 CUO 경로 기반으로 GGOCE 버전 자동 감지
  useEffect(() => {
    setGgoceVersion(null);
    const cuoPath = profile?.cuo_path;
    if (!cuoPath) return;
    let cancelled = false;
    api
      .detectGgoceVersion(cuoPath)
      .then((v) => !cancelled && setGgoceVersion(v))
      .catch(console.error);
    return () => {
      cancelled = true;
    };
  }, [profile?.cuo_path]);

  const onPluginsChange = (plugins: PluginEntry[]) => {
    if (!settings) return;
    persistSettings({ ...settings, plugins });
  };

  // ── 업데이트 버튼 ─────────────────────────────────
  const onUpdateClick = () => {
    if (updateState.kind === "available") {
      setUpdateState({ kind: "downloading", percent: 0 });
      let pct = 0;
      const iv = setInterval(() => {
        pct += 12;
        if (pct >= 100) {
          clearInterval(iv);
          setUpdateState({ kind: "ready" });
        } else {
          setUpdateState({ kind: "downloading", percent: pct });
        }
      }, 250);
    }
  };
  const onUpdateDoubleClick = () => {
    switch (updateState.kind) {
      case "uptodate":
        setUpdateState({ kind: "available", version: "1.5.0" });
        break;
      case "available":
      case "ready":
      case "error":
        setUpdateState({ kind: "uptodate" });
        break;
    }
  };

  let updateLabel: string;
  let updateClass: string;
  let updateDisabled = false;
  switch (updateState.kind) {
    case "checking":
      updateLabel = "업데이트 확인 중...";
      updateClass = "btn-update-checking";
      updateDisabled = true;
      break;
    case "uptodate":
      updateLabel = "최신 버전";
      updateClass = "btn-update-uptodate";
      updateDisabled = true;
      break;
    case "available":
      updateLabel = `업데이트 다운로드 v${updateState.version}`;
      updateClass = "btn-update-available";
      break;
    case "downloading":
      updateLabel = `다운로드 중 ${updateState.percent}%`;
      updateClass = "btn-update-downloading";
      updateDisabled = true;
      break;
    case "ready":
      updateLabel = "재시작하여 적용";
      updateClass = "btn-update-ready";
      break;
    case "error":
      updateLabel = "체크 실패 — 재시도";
      updateClass = "btn-update-error";
      break;
  }

  return (
    <div className="app">
      {/* ── Left ─────────────────────────── */}
      <aside className="left-column">
        <div className="logo-wrap">
          <img src="/margo-logo.png" alt="Margo Launcher" className="logo" />
          <div className="fan-made-note">* Unofficial fan-made launcher</div>
        </div>

        <div className="btn-group">
          <div className="btn-group-label">MARGO</div>
          <LinkButton label="Discord" url="https://discord.gg/VGfYrJFXtH" />
          <LinkButton label="Website" />
          <LinkButton label="오픈카톡" />
        </div>

        <div className="btn-group">
          <div className="btn-group-label">GGO SUPPORT</div>
          <LinkButton label="웹훅 발급소" url="https://discord.gg/KQzHZsZ9eH" />
          <LinkButton label="문의하기" url="https://open.kakao.com/o/sA71kz5d" />
        </div>

        <div className="btn-group">
          <div className="btn-group-label">ORIGINAL CLASSICUO</div>
          <LinkButton label="클래식유오" url="https://www.classicuo.eu" />
        </div>
      </aside>

      {/* ── Right ────────────────────────── */}
      <main className="right-column">
        <div className="top-actions">
          <button
            className={`btn-play ${!hasProfile ? "btn-play-disabled" : ""}`}
            disabled={!hasProfile}
            title={hasProfile ? "" : "프로필을 먼저 생성하세요"}
          >
            PLAY
          </button>
          <button
            className={`btn-update ${updateClass} ${updateDisabled ? "is-disabled" : ""}`}
            aria-disabled={updateDisabled}
            onClick={onUpdateClick}
            onDoubleClick={onUpdateDoubleClick}
            title="더블클릭으로 상태 토글 (개발용)"
          >
            {updateLabel}
            {updateState.kind === "downloading" && (
              <div
                className="btn-update-progress"
                style={{ width: `${updateState.percent}%` }}
              />
            )}
          </button>
        </div>

        <section className="profile-box">
          <header className="profile-header">
            <span className="profile-title">DESKTOP CLIENT SETTINGS</span>
            <span className="profile-version">
              ClassicUO GGO CE {ggoceVersion ? `v${ggoceVersion}` : "v—"}
            </span>
          </header>
          <div className="profile-body">
            <div className="profile-info">
              <div className="profile-name">
                {profile ? profile.name : "프로필 없음"}
              </div>
              <div className="profile-addr">
                {profile
                  ? `${profile.server.address}:${profile.server.port}`
                  : "—"}
              </div>
            </div>
            <button className="btn-change" onClick={() => setManageOpen(true)}>
              {profile ? "Change" : "New Profile"}
            </button>
          </div>
        </section>

        <PluginPanel
          plugins={settings?.plugins ?? []}
          onChange={onPluginsChange}
        />


        <section className="notice-row">
          <NoticeBoard source="margo" title="Margo 공지" />
          <NoticeBoard source="ggouo" title="GGOUO 공지" />
        </section>

        {loadError && (
          <div className="error-toast">설정 로드 실패: {loadError}</div>
        )}
      </main>

      {/* ── Modals ────────────────────────── */}
      {settings && (
        <>
          <ManageProfilesModal
            open={manageOpen}
            settings={settings}
            onClose={() => setManageOpen(false)}
            onChange={persistSettings}
            onEdit={(id) => setEditingId(id)}
          />
          <EditProfileModal
            open={editingId !== null}
            profile={
              editingId
                ? settings.profiles.find((p) => p.id === editingId) ?? null
                : null
            }
            onClose={() => setEditingId(null)}
            onSave={(updated) => {
              const next: Settings = {
                ...settings,
                profiles: settings.profiles.map((p) =>
                  p.id === updated.id ? updated : p
                ),
              };
              persistSettings(next);
            }}
          />
        </>
      )}
    </div>
  );
}

export default App;
