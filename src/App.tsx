import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { NoticeBoard } from "./NoticeBoard";
import { ManageProfilesModal } from "./ManageProfilesModal";
import { EditProfileModal } from "./EditProfileModal";
import { Modal } from "./Modal";
import { OnboardingBanner } from "./OnboardingBanner";
import { PluginPanel } from "./PluginPanel";
import { ServerStatusBadge } from "./ServerStatusBadge";
import { Stage0DiagnosticsPanel } from "./Stage0DiagnosticsPanel";
import type {
  GroupControlAction,
  LauncherManifest,
  MultiSessionStatus,
  NoticeBoard as NoticeBoardData,
  PluginEntry,
  Profile,
  Settings,
  Sidebar,
  UpdateCheck,
} from "./types";

/**
 * 툴팁 표준화 가이드 (모든 인터랙티브 요소에 적용):
 * - 활성 + 클릭 가능 → "[동작 설명]"             예: "프로필 편집"
 * - 비활성 + 이유    → "[이유 — 해결방법]"        예: "프로필을 먼저 생성하세요"
 * - 토글/체크박스    → "[현재 상태] — [반전 안내]" 예: "MULTI 포함됨 — 해제하려면 클릭"
 * - 상태 표시        → 그 자체로 충분 (별도 툴팁 생략)
 */

// fetch 실패 시 사용할 폴백 사이드바 (런처 초기 배포 시 박혀있던 값)
const FALLBACK_SIDEBAR: Sidebar = {
  groups: [
    {
      label: "MARGO",
      buttons: [
        { label: "설정 가이드", url: "https://docs.google.com/presentation/d/1iA6vzvoBJPdn_FnCCv4HwOVrhrgPSv4OAhWop-rcqAo/present?usp=sharing" },
        { label: "홈페이지", url: null },
        { label: "오픈카톡", url: null },
      ],
    },
    {
      label: "GGO SUPPORT",
      buttons: [
        { label: "웹훅 발급소", url: "https://discord.gg/KQzHZsZ9eH" },
        { label: "문의하기", url: "https://open.kakao.com/o/sA71kz5d" },
      ],
    },
    {
      label: "ORIGINAL CLASSICUO",
      buttons: [{ label: "클래식유오", url: "https://www.classicuo.eu" }],
    },
  ],
};

const isGuideButtonPosition = (groupIndex: number, buttonIndex: number) =>
  groupIndex === 0 && buttonIndex === 0;

type LinkButtonProps = {
  label: string;
  url?: string;
  /** 가이드 미열람 강조 (로컬) */
  highlight?: boolean;
  /** 비상 격상 (sidebar.json 원격): 펄스 강조 */
  remoteHighlight?: boolean;
  /** 비상 격상 (sidebar.json 원격): 빨간 배지 텍스트 */
  badge?: string;
  onActivate?: () => void;
};

function LinkButton({
  label,
  url,
  highlight,
  remoteHighlight,
  badge,
  onActivate,
}: LinkButtonProps) {
  const disabled = !url;
  const pulse = highlight || remoteHighlight;
  const onClick = () => {
    if (!url) return;
    onActivate?.();
    api.openExternal(url).catch(console.error);
  };
  return (
    <button
      className={`side-btn ${disabled ? "side-btn-disabled" : ""} ${
        pulse ? "side-btn-highlight" : ""
      }`}
      onClick={onClick}
      disabled={disabled}
      title={disabled ? "준비 중" : url}
    >
      {label}
      {disabled && <span className="badge-soon">준비중</span>}
      {/* 원격 배지(비상) 우선, 없으면 가이드 필독 배지 */}
      {!disabled && badge && <span className="badge-alert">{badge}</span>}
      {!disabled && !badge && highlight && (
        <span className="badge-must">필독</span>
      )}
    </button>
  );
}

function activeProfile(settings: Settings | null): Profile | null {
  if (!settings || !settings.active_profile_id) return null;
  return settings.profiles.find((p) => p.id === settings.active_profile_id) ?? null;
}

function isMultiEnabled(account: Profile["server"]["accounts"][number], index: number) {
  const v = account.multi_enabled;
  return v == null ? index < 6 : v === true;
}

function profileMultiStats(profile: Profile) {
  const selected = profile.server.accounts.filter(isMultiEnabled);
  return {
    total: profile.server.accounts.length,
    runnable: selected.slice(0, 6).length,
    selected: selected.length,
  };
}

function createInstallProfile(cuoPath: string, count: number): Profile {
  const id = `p_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
  return {
    id,
    name: count === 0 ? "GGO Custom" : `새 프로필 ${count + 1}`,
    uo_path: "",
    cuo_path: cuoPath,
    client_version: null,
    secondary_layout_preset: "two_by_two",
    server: {
      address: "login.uoserver.com",
      port: 2593,
      encryption: "auto",
      accounts: [],
      active_account_id: null,
    },
  };
}

const NOTICE_REFRESH_MS = 5 * 60 * 1000;
const PROFILE_PAGE_SIZE = 4;
/** 번들 기본 배경. sidebar.json의 background_url로 원격 교체 가능 (보험설계). */
const DEFAULT_BG = "/bg-default.jpg";

/** sidebar.json에는 외부 HTTPS 이미지만 허용한다. 잘못된 값은 번들 배경으로 폴백. */
function normalizeRemoteBackgroundUrl(value?: string | null): string | null {
  if (!value?.trim()) return null;
  try {
    const url = new URL(value.trim());
    return url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

type UpdateState =
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; version: string }
  // cuo_path 미설정 — manifest는 받았고 사용자가 "설치" 버튼 누르면 위치 선택부터 시작
  | { kind: "not_installed"; version: string }
  | { kind: "downloading"; percent: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: "checking" });
  const [updateRetryNonce, setUpdateRetryNonce] = useState(0);
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
  const [profilePage, setProfilePage] = useState(0);
  const [editingId, setEditingId] = useState<string | null>(null);
  // 새 프로필 드래프트 — 저장 누르기 전까지 settings 미반영. id는 미리 생성됨.
  const [draftProfile, setDraftProfile] = useState<Profile | null>(null);
  const [ggoceVersion, setGgoceVersion] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  // 사이드바 — 원격 fetch, 실패 시 FALLBACK 사용
  const [sidebar, setSidebar] = useState<Sidebar>(FALLBACK_SIDEBAR);
  // 배경 아트 — 원격 URL은 프리로드 성공 시에만 교체 (실패해도 기본 배경 유지)
  const [bgUrl, setBgUrl] = useState<string>(DEFAULT_BG);
  const [noticeBoard, setNoticeBoard] = useState<NoticeBoardData | null>(null);
  const [noticeError, setNoticeError] = useState<string | null>(null);
  const [noticeRetryNonce, setNoticeRetryNonce] = useState(0);
  const noticeBoardRef = useRef<NoticeBoardData | null>(null);
  // 긴급 공지 상단 배너 — 세션 내 닫은 공지 id (재시작 시 다시 표시: 비상 정보라 의도적)
  const [urgentDismissed, setUrgentDismissed] = useState<string[]>([]);

  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch((e) => console.warn("[app version]", e));
  }, []);

  // 온보딩 3단계 모두 충족 시 자동 영구 dismiss
  useEffect(() => {
    if (!settings || settings.ui?.onboarding_dismissed) return;
    const done =
      settings.plugins.length > 0 &&
      settings.profiles.length > 0 &&
      !!settings.ui?.first_launch_completed;
    if (done) {
      const next: Settings = {
        ...settings,
        ui: { ...settings.ui, onboarding_dismissed: true },
      };
      persistSettings(next);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    settings?.plugins.length,
    settings?.profiles.length,
    settings?.ui?.first_launch_completed,
    settings?.ui?.onboarding_dismissed,
  ]);

  // 시작 시 1회 sidebar fetch
  useEffect(() => {
    let cancelled = false;
    api
      .fetchSidebar()
      .then((s) => {
        if (cancelled) return;
        if (s.groups?.length > 0) setSidebar(s);
        const remoteBackground = normalizeRemoteBackgroundUrl(s.background_url);
        if (remoteBackground) {
          const img = new Image();
          img.onload = () => {
            if (!cancelled) setBgUrl(remoteBackground);
          };
          img.onerror = () =>
            console.warn("[sidebar background] 이미지 로드 실패", remoteBackground);
          img.src = remoteBackground;
        } else if (s.background_url) {
          console.warn("[sidebar background] HTTPS URL만 허용됩니다");
        }
      })
      .catch((e) => console.warn("[sidebar fetch]", e));
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const fetchNotice = (showError: boolean) => {
      api
        .fetchNotice()
        .then((board) => {
          if (cancelled) return;
          noticeBoardRef.current = board;
          setNoticeBoard(board);
          setNoticeError(null);
        })
        .catch((e) => {
          if (cancelled) return;
          if (showError || noticeBoardRef.current === null) {
            setNoticeError(String(e));
          } else {
            console.warn("[notice refresh]", e);
          }
        });
    };

    fetchNotice(true);
    const id = window.setInterval(() => fetchNotice(false), NOTICE_REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [noticeRetryNonce]);

  useEffect(() => {
    api
      .launcherInit()
      .then(setSettings)
      .catch((e) => setLoadError(String(e)));
  }, []);

  // 현재 사용 중인 프로필이 어느 페이지에 있든 메인에서 바로 확인되도록 맞춘다.
  useEffect(() => {
    if (!settings?.active_profile_id) return;
    const index = settings.profiles.findIndex(
      (profile) => profile.id === settings.active_profile_id
    );
    if (index >= 0) setProfilePage(Math.floor(index / PROFILE_PAGE_SIZE));
  }, [settings?.active_profile_id, settings?.profiles]);

  // 활성 프로필 cuo_path 기반으로 업데이트 체크 (프로필 변경 시 재실행)
  useEffect(() => {
    const activeId = settings?.active_profile_id;
    const cuoPath = settings?.profiles.find((p) => p.id === activeId)?.cuo_path;
    let cancelled = false;
    setUpdateState({ kind: "checking" });
    if (!cuoPath) {
      // cuo_path 미지정 — manifest만 받아서 신규 설치 안내
      api
        .cuoFetchManifestForInstall()
        .then((res) => {
          if (cancelled) return;
          pendingCheck.current = res;
          setUpdateState({ kind: "not_installed", version: res.remote_version });
        })
        .catch((e) => {
          if (!cancelled) setUpdateState({ kind: "error", message: String(e) });
        });
      return () => {
        cancelled = true;
      };
    }
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
  }, [settings?.active_profile_id, settings?.profiles, updateRetryNonce]);

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

  // 런처 자기 업데이트 체크 — 시작 시 1회 + 6시간마다 polling.
  useEffect(() => {
    let cancelled = false;
    const check = () => {
      api
        .launcherCheckUpdate()
        .then((res) => {
          if (cancelled) return;
          if (res.update_available && res.manifest) {
            setSelfUpdate((cur) =>
              cur.kind === "idle"
                ? { kind: "available", manifest: res.manifest! }
                : cur
            );
          }
        })
        .catch((e) => console.warn("[launcher check]", e));
    };
    check();
    const id = setInterval(check, 6 * 60 * 60 * 1000); // 6h
    return () => {
      cancelled = true;
      clearInterval(id);
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

  /** 설정 가이드를 1회 이상 열었다고 영구 기록 (사이드바 강조 해제). */
  const markGuideOpened = () => {
    if (!settings || settings.ui?.guide_opened) return;
    persistSettings({
      ...settings,
      ui: { ...settings.ui, guide_opened: true },
    });
  };

  // 최근 3일 내 urgent 공지 중 가장 최신 1건을 상단 배너로 격상.
  // (오래된 urgent가 공지 목록에 남아 있어도 배너로는 안 올라옴)
  const URGENT_BANNER_WINDOW_MS = 3 * 24 * 60 * 60 * 1000;
  const urgentNotice = (() => {
    if (!noticeBoard) return null;
    const all = [...(noticeBoard.margo ?? []), ...(noticeBoard.ggouo ?? [])];
    const now = Date.now();
    const fresh = all.filter((n) => {
      if (n.severity !== "urgent" || urgentDismissed.includes(n.id)) return false;
      const t = Date.parse(n.date);
      return !Number.isNaN(t) && now - t <= URGENT_BANNER_WINDOW_MS;
    });
    fresh.sort((a, b) => Date.parse(b.date) - Date.parse(a.date));
    return fresh[0] ?? null;
  })();

  const profile = activeProfile(settings);
  const hasProfile = !!profile;
  const hasActivePlugin = !!settings?.plugins.find((p) => p.enabled);
  const canPlay = hasProfile && hasActivePlugin;
  const accountCount = profile?.server.accounts.length ?? 0;
  // MULTI 대상 카운트 (레거시: 인덱스<6이면 true)
  const multiCount = profile
    ? profile.server.accounts.filter((a, i) => {
        const v = a.multi_enabled;
        return v == null ? i < 6 : v === true;
      }).slice(0, 6).length
    : 0;
  const multiSelectedTotal = profile
    ? profile.server.accounts.filter((a, i) => {
        const v = a.multi_enabled;
        return v == null ? i < 6 : v === true;
      }).length
    : 0;
  const managedSecondaryCount = profile
    ? profile.server.accounts.filter((account, index) => {
        const enabled = account.multi_enabled == null ? index < 6 : account.multi_enabled;
        return enabled && account.secondary_slot != null;
      }).slice(0, 5).length
    : 0;
  const canMultiLogin = canPlay && multiCount > 0;
  const totalProfilePages = Math.max(
    1,
    Math.ceil((settings?.profiles.length ?? 0) / PROFILE_PAGE_SIZE)
  );
  const visibleProfilePage = Math.min(profilePage, totalProfilePages - 1);
  const profileOptions =
    settings?.profiles.slice(
      visibleProfilePage * PROFILE_PAGE_SIZE,
      (visibleProfilePage + 1) * PROFILE_PAGE_SIZE
    ) ?? [];
  const hasAnyProfile = (settings?.profiles.length ?? 0) > 0;

  const [launching, setLaunching] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [launchInfo, setLaunchInfo] = useState<string | null>(null);
  const [multiStatus, setMultiStatus] = useState<MultiSessionStatus | null>(null);
  const [groupControlling, setGroupControlling] = useState<GroupControlAction | null>(null);

  const refreshMultiStatus = async (profileId: string) => {
    try {
      const status = await api.multiclientSessionStatus(profileId);
      setMultiStatus(status);
    } catch {
      // 상태 표시는 보조 정보다. 실행 오류와 섞지 않고 다음 poll에서 재시도한다.
    }
  };

  useEffect(() => {
    const profileId = profile?.id;
    if (!profileId) {
      setMultiStatus(null);
      return;
    }
    let disposed = false;
    const refresh = async () => {
      try {
        const status = await api.multiclientSessionStatus(profileId);
        if (!disposed) setMultiStatus(status);
      } catch {
        // 런처 초기화/프로필 전환 순간의 일시 실패는 다음 poll에서 회복한다.
      }
    };
    void refresh();
    const timer = setInterval(refresh, 2000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [profile?.id, multiCount]);

  // PLAY (단일, 무인증 — CUO 로그인 화면에서 사용자가 입력)
  const onPlay = async () => {
    if (!profile) return;
    setLaunching(true);
    setLaunchError(null);
    setLaunchInfo(null);
    try {
      await api.cuoLaunch(profile.id, null);
      // 백엔드가 first_launch_completed를 마킹했으므로 settings 재로드
      // (온보딩 배너 3단계 자동 반영)
      try {
        const fresh = await api.getSettings();
        setSettings(fresh);
      } catch {
        /* settings reload 실패는 silent */
      }
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
      setLaunchError("등록된 계정이 없습니다. 프로필 편집에서 추가하세요.");
      return;
    }
    setLaunching(true);
    setLaunchError(null);
    setLaunchInfo(null);
    try {
      const result = await api.cuoLaunchMulti(profile.id, 4000);
      const summary = `신규 실행 ${result.launched_count}개 / 기존 유지 ${result.already_running_count}개`;
      setLaunchInfo(
        result.layout_warning ? `${summary} · ${result.layout_warning}` : summary
      );
      await refreshMultiStatus(profile.id);
      // 백엔드가 first_launch_completed를 마킹했으므로 settings 재로드
      try {
        const fresh = await api.getSettings();
        setSettings(fresh);
      } catch {
        /* settings reload 실패는 silent */
      }
    } catch (e) {
      setLaunchError(String(e));
    } finally {
      setLaunching(false);
    }
  };

  const onGroupControl = async (action: GroupControlAction) => {
    if (!profile) return;
    setGroupControlling(action);
    setLaunchError(null);
    setLaunchInfo(null);
    try {
      const result = await api.multiclientGroupControl(profile.id, action);
      const label =
        action === "minimize"
          ? "보조창 최소화"
          : action === "restore_preset"
          ? "보조창 복원·재배치"
          : action === "group_raise"
          ? "보조창 앞으로 모으기"
          : "보조 클라이언트 종료";
      setLaunchInfo(
        `${label}: 성공 ${result.succeeded_count}개` +
          (result.pending_count > 0 ? ` · 준비 중 ${result.pending_count}개` : "") +
          (result.failed_count > 0 ? ` · 실패 ${result.failed_count}개` : "")
      );
    } catch (e) {
      setLaunchError(String(e));
    } finally {
      setGroupControlling(null);
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

  const onSelectProfile = (profileId: string) => {
    if (!settings || settings.active_profile_id === profileId) return;
    persistSettings({ ...settings, active_profile_id: profileId });
  };

  // ── 설치/업데이트 모달 상태 ────────────────────────
  // 신규 설치 위치 선택 모달 — 부모 폴더 + 하위 폴더명 분리 입력
  const [installPickerOpen, setInstallPickerOpen] = useState(false);
  const [installParent, setInstallParent] = useState<string>("");
  const [installSubfolder, setInstallSubfolder] =
    useState<string>("ClassicUO-GGOCE");
  // 원본 CUO 덮어쓰기 확인 모달
  const [originalCuoConfirm, setOriginalCuoConfirm] = useState<{
    cuoPath: string;
  } | null>(null);

  // OS path separator 감지 (Windows = \, 그 외 = /)
  const pathSep = (p: string) => (p.includes("\\") ? "\\" : "/");
  const joinPath = (parent: string, sub: string) => {
    if (!parent) return sub;
    const sep = pathSep(parent);
    const cleanParent = parent.replace(/[\\/]+$/, "");
    return `${cleanParent}${sep}${sub}`;
  };
  const installFinalPath =
    installParent && installSubfolder.trim()
      ? joinPath(installParent, installSubfolder.trim())
      : "";
  // 폴더명 sanitize — 파일시스템 invalid 문자 차단
  const subfolderInvalid =
    installSubfolder.trim().length === 0 ||
    /[\\/:*?"<>|]/.test(installSubfolder) ||
    installSubfolder.trim() === "." ||
    installSubfolder.trim() === "..";

  // 실제 다운로드 실행 — pendingCheck + cuoPath + allow 플래그로 동작
  const runApply = async (cuoPath: string, allowOriginalOverwrite: boolean) => {
    const check = pendingCheck.current;
    if (!check) return;
    setUpdateState({ kind: "downloading", percent: 0 });
    try {
      await api.cuoApplyUpdate(cuoPath, check, allowOriginalOverwrite);
      pendingCheck.current = null;
      setUpdateState({ kind: "uptodate" });
    } catch (e) {
      setUpdateState({ kind: "error", message: String(e) });
    }
  };

  // ── 업데이트 버튼 ─────────────────────────────────
  const onUpdateClick = async () => {
    // 신규 설치 — 위치 선택 모달 오픈 (기본: 런처 디렉터리)
    if (updateState.kind === "not_installed") {
      try {
        const dir = await api.getLauncherDir();
        setInstallParent(dir ?? "");
      } catch {
        setInstallParent("");
      }
      setInstallSubfolder("ClassicUO-GGOCE");
      setInstallPickerOpen(true);
      return;
    }

    if (updateState.kind !== "available") return;
    const cuoPath = profile?.cuo_path ?? "";
    if (!cuoPath || !pendingCheck.current) return;

    // 원본 CUO 폴더 감지 → 확인 모달
    try {
      const kind = await api.detectFolderKind(cuoPath);
      if (kind.kind === "original_cuo") {
        setOriginalCuoConfirm({ cuoPath });
        return;
      }
    } catch (e) {
      console.warn("detect_folder_kind 실패, 그대로 진행:", e);
    }
    await runApply(cuoPath, false);
  };

  // 신규 설치 — 사용자가 위치 선택 완료
  const onInstallTo = async (targetPath: string) => {
    if (!settings) return;
    setInstallPickerOpen(false);
    const currentProfile = profile;
    const updatedProfile: Profile | null = currentProfile
      ? { ...currentProfile, cuo_path: targetPath }
      : null;
    const createdProfile = updatedProfile
      ? null
      : createInstallProfile(targetPath, settings.profiles.length);
    const installProfile = updatedProfile ?? createdProfile!;
    const nextProfiles = updatedProfile
      ? settings.profiles.map((p) =>
          p.id === updatedProfile.id ? updatedProfile : p
        )
      : [...settings.profiles, installProfile];
    const next: Settings = {
      ...settings,
      profiles: nextProfiles,
      active_profile_id: settings.active_profile_id ?? installProfile.id,
    };
    persistSettings(next);
    // 빈 폴더 또는 새 폴더 → 원본 CUO 아님, allow_original_overwrite=false 면 충분
    await runApply(targetPath, false);
  };

  const onPickParentFolder = async () => {
    const picked = await api.cuoSelectDirectory(installParent || undefined);
    if (picked) setInstallParent(picked);
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
    case "not_installed":
      updateLabel = `GGO CE 설치 v${updateState.version}`;
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
      {/* 배경 키 아트 (오버레이는 CSS ::after) */}
      <div
        className="app-bg"
        style={{ backgroundImage: `url("${bgUrl}")` }}
        aria-hidden
      />

      {/* ── Left ─────────────────────────── */}
      <aside className="left-column">
        <div className="logo-wrap">
          <img src="/margo-logo.png" alt="Margo Launcher" className="logo" />
        </div>

        <ServerStatusBadge />

        {sidebar.groups.map((g, groupIndex) => (
          <div key={g.label} className="btn-group">
            <div className="btn-group-label">{g.label}</div>
            {g.buttons.map((b, buttonIndex) => {
              const isGuide = isGuideButtonPosition(groupIndex, buttonIndex);
              return (
                <LinkButton
                  key={`${b.label}-${buttonIndex}`}
                  label={b.label}
                  url={b.url ?? undefined}
                  highlight={isGuide && !settings?.ui?.guide_opened}
                  remoteHighlight={b.highlight === true}
                  badge={b.badge ?? undefined}
                  onActivate={isGuide ? markGuideOpened : undefined}
                />
              );
            })}
          </div>
        ))}
      </aside>

      {/* ── Right ────────────────────────── */}
      <main className="right-column">
        {/* 긴급 배너는 표시 전용. 실제 비상 시 클릭 연결이 필요해지면:
            공지에 url 필드를 넣고 아래 div에
            onClick={() => urgentNotice.url && api.openExternal(urgentNotice.url)}
            + style={{ cursor: "pointer" }} 를 추가하면 됨 (✕ 버튼은 stopPropagation 필요). */}
        {urgentNotice && (
          <div className="urgent-banner" role="alert">
            <span className="urgent-banner-label">긴급</span>
            <span className="urgent-banner-msg" title={urgentNotice.title}>
              {urgentNotice.title}
            </span>
            <span className="urgent-banner-date">{urgentNotice.date}</span>
            <button
              className="urgent-banner-close"
              onClick={() =>
                setUrgentDismissed((d) => [...d, urgentNotice.id])
              }
              aria-label="긴급 배너 닫기"
              title="닫기 (공지 목록에는 그대로 남음)"
            >
              ✕
            </button>
          </div>
        )}
        {settings && !settings.ui?.onboarding_dismissed && (
          <OnboardingBanner
            settings={settings}
            onDismiss={() => {
              const next: Settings = {
                ...settings,
                ui: { ...settings.ui, onboarding_dismissed: true },
              };
              persistSettings(next);
            }}
          />
        )}
        {selfUpdate.kind !== "idle" && (
          <div className={`self-update-banner self-update-${selfUpdate.kind}`}>
            {selfUpdate.kind === "available" && (
              <>
                <span className="self-update-msg self-update-msg-stacked">
                  <span>
                    새 런처 버전 <b>v{selfUpdate.manifest.version}</b> 사용 가능
                  </span>
                  {selfUpdate.manifest.notes && (
                    <span className="self-update-notes">
                      {selfUpdate.manifest.notes}
                    </span>
                  )}
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
                : multiCount === 0
                ? "프로필 편집에서 MULTI 대상 계정을 선택하세요"
                : `${multiCount}개 계정 순차 자동 로그인`
            }
          >
            MULTI LOGIN
            <div className="btn-sublabel">
              {multiCount === 0
                ? "선택 0개"
                : multiStatus && multiStatus.selected_count === multiCount
                ? multiStatus.missing_count === 0
                  ? `${multiCount}/${multiCount} 실행 중`
                  : multiStatus.active_count +
                      multiStatus.pending_count +
                      multiStatus.untracked_count ===
                    0
                  ? `${multiCount}개 실행`
                  : `${
                      multiStatus.active_count +
                      multiStatus.pending_count +
                      multiStatus.untracked_count
                    }/${multiCount} 실행 중 · ${multiStatus.missing_count}개 복구`
                : multiSelectedTotal > 6
                ? `${multiCount}/6개 실행 / ${accountCount}계정`
                : `${multiCount}개 실행`}
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
            <span className="profile-title-group">
              <span className="profile-title">빠른 프로필</span>
              {profile && (
                <span className="profile-current" title={`현재 사용 중: ${profile.name}`}>
                  현재: {profile.name}
                </span>
              )}
            </span>
            {managedSecondaryCount > 0 && (
              <div
                className="secondary-group-controls"
                aria-label="보조 모니터 게임창 그룹 제어"
              >
                <span className="secondary-group-label">
                  보조 {managedSecondaryCount}
                </span>
                <button
                  className="secondary-group-button"
                  disabled={groupControlling !== null}
                  onClick={() => onGroupControl("minimize")}
                  title="숨: 보조 모니터에 지정한 게임창만 한꺼번에 최소화"
                  aria-label="보조창 전체 숨김"
                >
                  숨
                </button>
                <button
                  className="secondary-group-button"
                  disabled={groupControlling !== null}
                  onClick={() => onGroupControl("restore_preset")}
                  title="복: 보조 게임창을 복원하고 프리셋 위치와 크기를 다시 적용"
                  aria-label="보조창 위치 및 크기 복원"
                >
                  복
                </button>
                <button
                  className="secondary-group-button"
                  disabled={groupControlling !== null}
                  onClick={() => onGroupControl("group_raise")}
                  title="앞: 포커스를 바꾸지 않고 보조 게임창 그룹을 다른 창 앞으로 올림"
                  aria-label="보조창 전체 앞으로"
                >
                  앞
                </button>
              </div>
            )}
            <span className="profile-version">
              ClassicUO GGO CE {ggoceVersion ? `v${ggoceVersion}` : "v—"}
            </span>
          </header>
          <div className="profile-body">
            {settings && profileOptions.length > 0 ? (
              <>
                <div
                  className="profile-quick-grid"
                  style={{
                    gridTemplateColumns: `repeat(${Math.max(
                      1,
                      profileOptions.length
                    )}, minmax(0, 1fr))`,
                  }}
                >
                  {profileOptions.map((p) => {
                    const stats = profileMultiStats(p);
                    const isActive = p.id === settings.active_profile_id;
                    return (
                      <button
                        key={p.id}
                        type="button"
                        className={`profile-quick-card ${isActive ? "is-active" : ""}`}
                        onClick={() => onSelectProfile(p.id)}
                        aria-pressed={isActive}
                        title={isActive ? "현재 선택된 프로필" : "이 프로필 선택"}
                      >
                        <span className="profile-quick-name">
                          {p.name}
                        </span>
                        <span className="profile-quick-meta">
                          {stats.total > 0
                            ? `${stats.runnable}/${stats.total} 계정`
                            : "등록 계정 없음"}
                        </span>
                      </button>
                    );
                  })}
                </div>
                <div className="profile-side-controls">
                  <button
                    className="btn-change"
                    onClick={() => setManageOpen(true)}
                    title="프로필 전환 / 추가 / 편집"
                  >
                    프로필 관리
                  </button>
                  {totalProfilePages > 1 && (
                    <div className="profile-page-controls">
                    <button
                      type="button"
                      className="btn-profile-page"
                      onClick={() =>
                        setProfilePage(
                          (visibleProfilePage - 1 + totalProfilePages) %
                            totalProfilePages
                        )
                      }
                      title="이전 프로필 묶음"
                      aria-label="이전 프로필 묶음"
                    >
                      ‹
                    </button>
                    <button
                      type="button"
                      className="btn-profile-page"
                      onClick={() =>
                        setProfilePage(
                          (visibleProfilePage + 1) % totalProfilePages
                        )
                      }
                      title="다음 프로필 묶음"
                      aria-label="다음 프로필 묶음"
                    >
                      ›
                    </button>
                    </div>
                  )}
                </div>
              </>
            ) : hasAnyProfile ? (
              <div className="profile-empty">
                <div className="profile-empty-mark">✦</div>
                <div className="profile-empty-title">메인에 표시할 프로필을 선택하세요</div>
                <div className="profile-empty-hint">
                  프로필 관리에서 순서를 정하면 앞의 프로필부터 이곳에 표시됩니다.
                </div>
                <button
                  className="btn-primary btn-primary-sm"
                  onClick={() => setManageOpen(true)}
                  title="프로필 관리 모달을 열어 메인 표시 프로필 선택"
                  style={{ marginTop: 4 }}
                >
                  프로필 관리
                </button>
              </div>
            ) : (
              <div className="profile-empty">
                <div className="profile-empty-mark">✦</div>
                <div className="profile-empty-title">첫 프로필을 만들어볼까요?</div>
                <div className="profile-empty-hint">
                  접속 서버 정보와 계정을 등록하면 바로 PLAY/MULTI LOGIN 가능합니다.
                </div>
                <button
                  className="btn-primary btn-primary-sm"
                  onClick={() => setManageOpen(true)}
                  title="프로필 관리 모달을 열어 새 프로필 생성"
                  style={{ marginTop: 4 }}
                >
                  + 프로필 만들기
                </button>
              </div>
            )}
          </div>
        </section>

        <PluginPanel
          plugins={settings?.plugins ?? []}
          onChange={onPluginsChange}
        />


        <section className="notice-row">
          <NoticeBoard
            title="Margo 공지"
            items={noticeBoard?.margo ?? null}
            loading={!noticeError && noticeBoard === null}
            error={noticeError}
            onRetry={() => setNoticeRetryNonce((n) => n + 1)}
          />
          <NoticeBoard
            title="GGOUO 공지"
            items={noticeBoard?.ggouo ?? null}
            loading={!noticeError && noticeBoard === null}
            error={noticeError}
            onRetry={() => setNoticeRetryNonce((n) => n + 1)}
          />
        </section>

        {loadError && (
          <div className="error-toast">설정 로드 실패: {loadError}</div>
        )}
        {updateState.kind === "error" && (
          <div
            className="error-toast"
            style={{ whiteSpace: "pre-wrap" }}
            onClick={() => {
              // 재체크 트리거
              setUpdateRetryNonce((n) => n + 1);
            }}
          >
            업데이트 처리 실패 — 클릭하면 재시도{"\n"}
            <span style={{ fontSize: 11, opacity: 0.85, fontFamily: "ui-monospace, Consolas, monospace" }}>
              {updateState.message}
            </span>
          </div>
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

      <Stage0DiagnosticsPanel />

      {/* ── Modals ────────────────────────── */}
      {settings && (
        <>
          <ManageProfilesModal
            open={manageOpen}
            settings={settings}
            onClose={() => setManageOpen(false)}
            onChange={persistSettings}
            onEdit={(id) => setEditingId(id)}
            onCreate={(draft) => setDraftProfile(draft)}
          />
          <EditProfileModal
            open={editingId !== null || draftProfile !== null}
            profile={
              draftProfile
                ? draftProfile
                : editingId
                ? settings.profiles.find((p) => p.id === editingId) ?? null
                : null
            }
            onClose={() => {
              setEditingId(null);
              setDraftProfile(null);
            }}
            onSave={(updated) => {
              const exists = settings.profiles.some((p) => p.id === updated.id);
              const profiles = exists
                ? settings.profiles.map((p) =>
                    p.id === updated.id ? updated : p
                  )
                : [...settings.profiles, updated];
              const active_profile_id =
                settings.active_profile_id ??
                profiles[0]?.id ??
                updated.id;
              const next: Settings = {
                ...settings,
                profiles,
                active_profile_id,
              };
              persistSettings(next);
              setDraftProfile(null);
            }}
          />
        </>
      )}

      {/* 신규 설치 위치 선택 모달 — 부모 폴더 + 하위 폴더명 */}
      {installPickerOpen && (
        <Modal
          open={installPickerOpen}
          onClose={() => setInstallPickerOpen(false)}
          title="GGO CE 설치 위치"
          width={560}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <p style={{ margin: 0, lineHeight: 1.5 }}>
              부모 폴더 안에 하위 폴더를 만들고 그 안에 GGO CE 본체를 설치합니다.
              기본값은 런처가 있는 위치 옆입니다.
            </p>

            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <label
                style={{ fontSize: 12, opacity: 0.8 }}
                htmlFor="install-parent"
              >
                부모 폴더
              </label>
              <div style={{ display: "flex", gap: 6 }}>
                <input
                  id="install-parent"
                  className="text-input"
                  style={{ flex: 1, fontFamily: "ui-monospace, Consolas, monospace", fontSize: 12 }}
                  value={installParent}
                  onChange={(e) => setInstallParent(e.target.value)}
                  placeholder="C:\\..."
                />
                <button className="btn-action" onClick={onPickParentFolder}>
                  찾기
                </button>
              </div>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <label
                style={{ fontSize: 12, opacity: 0.8 }}
                htmlFor="install-subfolder"
              >
                하위 폴더 이름
              </label>
              <input
                id="install-subfolder"
                className="text-input"
                value={installSubfolder}
                onChange={(e) => setInstallSubfolder(e.target.value)}
                placeholder="ClassicUO-GGOCE"
              />
              {subfolderInvalid && (
                <span style={{ fontSize: 11, color: "#f88" }}>
                  유효하지 않은 폴더명입니다 (특수문자 \\ / : * ? " &lt; &gt; | 사용 불가)
                </span>
              )}
            </div>

            <div className="modal-path-box">
              <div className="modal-path-label">최종 설치 경로</div>
              <div className="modal-path-value">{installFinalPath || "—"}</div>
            </div>

            <div
              style={{
                display: "flex",
                gap: 8,
                justifyContent: "flex-end",
                marginTop: 4,
              }}
            >
              <button
                className="btn-action"
                onClick={() => setInstallPickerOpen(false)}
              >
                취소
              </button>
              <button
                className="btn-primary btn-primary-sm"
                disabled={
                  !installParent || subfolderInvalid || !installFinalPath
                }
                onClick={() => {
                  setInstallPickerOpen(false);
                  onInstallTo(installFinalPath);
                }}
              >
                설치
              </button>
            </div>
          </div>
        </Modal>
      )}

      {/* 원본 CUO 덮어쓰기 확인 모달 */}
      {originalCuoConfirm && (
        <Modal
          open={originalCuoConfirm !== null}
          onClose={() => setOriginalCuoConfirm(null)}
          title="원본 ClassicUO 폴더 감지"
          width={560}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <p style={{ margin: 0, lineHeight: 1.5 }}>
              선택한 폴더는 GGO CE가 아닌 <b>원본 ClassicUO</b> 폴더로 보입니다.
              계속 진행하면 ClassicUO.exe와 일부 DLL이 GGO CE 빌드로
              교체됩니다. (기존 파일은 같은 위치에 <code>.bak</code>로
              백업됩니다.)
            </p>
            <div className="modal-path-box">
              <div className="modal-path-value">{originalCuoConfirm.cuoPath}</div>
            </div>
            <p style={{ margin: 0, fontSize: 13, opacity: 0.8 }}>
              계속 진행할까요?
            </p>
            <div
              style={{
                display: "flex",
                gap: 8,
                justifyContent: "flex-end",
                marginTop: 4,
              }}
            >
              <button
                className="btn-action"
                onClick={() => setOriginalCuoConfirm(null)}
              >
                취소
              </button>
              <button
                className="btn-action btn-action-danger"
                onClick={async () => {
                  const path = originalCuoConfirm.cuoPath;
                  setOriginalCuoConfirm(null);
                  await runApply(path, true);
                }}
              >
                계속 진행 (덮어쓰기)
              </button>
            </div>
          </div>
        </Modal>
      )}

      <div className="app-footer">
        <span>* Unofficial fan-made launcher</span>
        {appVersion && <span> · Launcher v{appVersion}</span>}
      </div>
    </div>
  );
}

export default App;
