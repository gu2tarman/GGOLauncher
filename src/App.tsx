import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { NoticeBoard } from "./NoticeBoard";
import { ManageProfilesModal } from "./ManageProfilesModal";
import { EditProfileModal } from "./EditProfileModal";
import { PluginPanel } from "./PluginPanel";
import type {
  LauncherManifest,
  PluginEntry,
  Profile,
  Settings,
  UpdateCheck,
} from "./types";

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
  // 사용 가능한 manifest 보관 (다운로드 시 재사용 — 중복 fetch 회피)
  const pendingCheck = useRef<UpdateCheck | null>(null);

  // 런처 자기 업데이트 state
  type SelfUpdate =
    | { kind: "idle" }
    | { kind: "available"; manifest: LauncherManifest }
    | { kind: "downloading"; percent: number }
    | { kind: "applying" }
    | { kind: "error"; message: string };
  const [selfUpdate, setSelfUpdate] = useState<SelfUpdate>({ kind: "idle" });
  const [manageOpen, setManageOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [ggoceVersion, setGgoceVersion] = useState<string | null>(null);

  useEffect(() => {
    api
      .launcherInit()
      .then(setSettings)
      .catch((e) => setLoadError(String(e)));
  }, []);

  // 활성 프로필 cuo_path 기반으로 업데이트 체크 (프로필 변경 시 재실행)
  useEffect(() => {
    const activeId = settings?.active_profile_id;
    const cuoPath = settings?.profiles.find((p) => p.id === activeId)?.cuo_path;
    if (!cuoPath) {
      setUpdateState({ kind: "uptodate" });
      return;
    }
    let cancelled = false;
    setUpdateState({ kind: "checking" });
    api
      .cuoCheckUpdate(cuoPath)
      .then((res) => {
        if (cancelled) return;
        if (res.changed.length === 0) {
          pendingCheck.current = null;
          setUpdateState({ kind: "uptodate" });
        } else {
          pendingCheck.current = res;
          setUpdateState({ kind: "available", version: res.remote_version });
        }
      })
      .catch((e) => {
        if (!cancelled) setUpdateState({ kind: "error", message: String(e) });
      });
    return () => {
      cancelled = true;
    };
  }, [settings?.active_profile_id, settings?.profiles]);

  // CUO 업데이트 다운로드 진행률
  useEffect(() => {
    const unlistenPromise = listen<{ bytesDone: number; totalBytes: number }>(
      "cuo_update_progress",
      (e) => {
        const { bytesDone, totalBytes } = e.payload;
        const percent = totalBytes > 0 ? Math.floor((bytesDone / totalBytes) * 100) : 0;
        setUpdateState({ kind: "downloading", percent });
      }
    );
    return () => {
      unlistenPromise.then((u) => u());
    };
  }, []);

  // 런처 자기 업데이트 체크 (시작 시 1회)
  useEffect(() => {
    let cancelled = false;
    api
      .launcherCheckUpdate()
      .then((res) => {
        if (cancelled) return;
        if (res.update_available && res.manifest) {
          setSelfUpdate({ kind: "available", manifest: res.manifest });
        }
      })
      .catch((e) => console.error("[launcher check]", e));
    return () => {
      cancelled = true;
    };
  }, []);

  // 런처 자기 업데이트 진행률
  useEffect(() => {
    const unlistenPromise = listen<{ bytesDone: number; totalBytes: number }>(
      "launcher_update_progress",
      (e) => {
        const { bytesDone, totalBytes } = e.payload;
        const percent = totalBytes > 0 ? Math.floor((bytesDone / totalBytes) * 100) : 0;
        setSelfUpdate({ kind: "downloading", percent });
      }
    );
    return () => {
      unlistenPromise.then((u) => u());
    };
  }, []);

  const onSelfUpdateClick = async () => {
    const manifest =
      selfUpdate.kind === "available" ? selfUpdate.manifest : null;
    if (!manifest) return;
    setSelfUpdate({ kind: "downloading", percent: 0 });
    try {
      await api.launcherApplyUpdate(manifest);
      setSelfUpdate({ kind: "applying" });
      // updater.bat이 swap+restart 담당 — 런처 자기 종료
      setTimeout(() => api.quitLauncher().catch(() => {}), 300);
    } catch (e) {
      setSelfUpdate({ kind: "error", message: String(e) });
    }
  };

  /** settings를 갱신하고 디스크에도 저장. */
  const persistSettings = (next: Settings) => {
    setSettings(next);
    api.saveSettings(next).catch((e) => console.error("save failed:", e));
  };

  const profile = activeProfile(settings);
  const hasProfile = !!profile;
  const hasActivePlugin = !!settings?.plugins.find((p) => p.enabled);
  const canPlay = hasProfile && hasActivePlugin;
  const accountCount = profile?.server.accounts.length ?? 0;
  const activeAccount = profile?.server.accounts.find(
    (a) => a.id === profile.server.active_account_id
  );
  const canMultiLogin = canPlay && accountCount > 0;

  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launchInfo, setLaunchInfo] = useState<string | null>(null);

  // PLAY (단일, 무인증 — CUO 로그인 화면에서 사용자가 입력)
  const onPlay = async () => {
    if (!profile) return;
    setLaunching(true);
    setLaunchError(null);
    setLaunchInfo(null);
    try {
      await api.cuoLaunch(profile.id, null);
    } catch (e) {
      setLaunchError(String(e));
    } finally {
      setLaunching(false);
    }
  };

  // MULTI LOGIN (프로필 안 모든 계정 순차 자동 로그인)
  const onMultiLogin = async () => {
    if (!profile) return;
    if (accountCount === 0) {
      setLaunchError("등록된 계정이 없습니다. Edit Profile에서 추가하세요.");
      return;
    }
    setLaunching(true);
    setLaunchError(null);
    setLaunchInfo(null);
    try {
      const count = await api.cuoLaunchMulti(profile.id, 4000);
      setLaunchInfo(`${count}개 계정 순차 실행 중...`);
    } catch (e) {
      setLaunchError(String(e));
    } finally {
      setLaunching(false);
    }
  };

  // 에러 토스트 5초 자동 닫힘
  useEffect(() => {
    if (!launchError) return;
    const t = setTimeout(() => setLaunchError(null), 5000);
    return () => clearTimeout(t);
  }, [launchError]);

  useEffect(() => {
    if (!launchInfo) return;
    const t = setTimeout(() => setLaunchInfo(null), 4000);
    return () => clearTimeout(t);
  }, [launchInfo]);

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
  const onUpdateClick = async () => {
    if (updateState.kind !== "available") return;
    const check = pendingCheck.current;
    const cuoPath = profile?.cuo_path ?? "";
    if (!check || !cuoPath) return;
    setUpdateState({ kind: "downloading", percent: 0 });
    try {
      await api.cuoApplyUpdate(cuoPath, check);
      // 적용 완료 → 재체크 (모두 일치하면 uptodate)
      pendingCheck.current = null;
      setUpdateState({ kind: "uptodate" });
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
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
        {selfUpdate.kind !== "idle" && (
          <div className={`self-update-banner self-update-${selfUpdate.kind}`}>
            {selfUpdate.kind === "available" && (
              <>
                <span className="self-update-msg">
                  🆙 새 런처 버전 <b>v{selfUpdate.manifest.version}</b> 사용 가능
                </span>
                <button className="btn-primary btn-primary-sm" onClick={onSelfUpdateClick}>
                  업데이트 및 재시작
                </button>
              </>
            )}
            {selfUpdate.kind === "downloading" && (
              <span className="self-update-msg">
                런처 업데이트 다운로드 중 {selfUpdate.percent}%...
              </span>
            )}
            {selfUpdate.kind === "applying" && (
              <span className="self-update-msg">
                업데이트 적용 — 런처가 곧 재시작됩니다...
              </span>
            )}
            {selfUpdate.kind === "error" && (
              <>
                <span className="self-update-msg">
                  런처 업데이트 실패: {selfUpdate.message}
                </span>
                <button
                  className="btn-action"
                  onClick={() => setSelfUpdate({ kind: "idle" })}
                >
                  닫기
                </button>
              </>
            )}
          </div>
        )}
        <div className="top-actions top-actions-3col">
          <button
            className={`btn-play ${!canPlay ? "btn-play-disabled" : ""}`}
            disabled={!canPlay || launching}
            onClick={onPlay}
            title={
              !hasProfile
                ? "프로필을 먼저 생성하세요"
                : !hasActivePlugin
                ? "플러그인을 먼저 등록·선택하세요"
                : "CUO 로그인 화면에서 계정 직접 입력"
            }
          >
            PLAY
            <div className="btn-sublabel">수동 로그인</div>
          </button>
          <button
            className={`btn-multi ${!canMultiLogin ? "btn-multi-disabled" : ""}`}
            disabled={!canMultiLogin || launching}
            onClick={onMultiLogin}
            title={
              !hasProfile
                ? "프로필을 먼저 생성하세요"
                : !hasActivePlugin
                ? "플러그인을 먼저 등록·선택하세요"
                : accountCount === 0
                ? "계정을 먼저 등록하세요"
                : `${accountCount}개 계정 순차 자동 로그인`
            }
          >
            MULTI LOGIN
            <div className="btn-sublabel">
              {accountCount > 0 ? `${accountCount}개 계정 순차` : "계정 없음"}
            </div>
          </button>
          <button
            className={`btn-update ${updateClass} ${updateDisabled ? "is-disabled" : ""}`}
            aria-disabled={updateDisabled}
            onClick={onUpdateClick}
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
                {profile && accountCount > 0 && (
                  <span className="profile-account-pill">
                    {activeAccount?.username || "계정 미선택"}
                    {accountCount > 1 && ` · 총 ${accountCount}계정`}
                  </span>
                )}
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
        {launchError && (
          <div className="error-toast" onClick={() => setLaunchError(null)}>
            실행 실패: {launchError}
          </div>
        )}
        {launchInfo && (
          <div className="info-toast" onClick={() => setLaunchInfo(null)}>
            {launchInfo}
          </div>
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
