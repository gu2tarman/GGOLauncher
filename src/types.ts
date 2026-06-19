// Rust 측 모델과 1:1 미러. 변경 시 양쪽 같이 수정.

export type EncryptionType =
  | "auto"
  | "none"
  | "old_blowfish"
  | "blowfish125"
  | "twofish";

export interface Account {
  id: string;
  username: string;
  password_encrypted: string;
  /** 선택 캐릭터명. 비어 있으면 서버 선택 단계까지만 자동 진행 */
  character_name?: string | null;
  display_name?: string | null;
  /** MULTI LOGIN 포함 여부. null/undefined = 레거시(인덱스<6이면 true) */
  multi_enabled?: boolean | null;
}

export interface CuoProfileCandidate {
  account: string;
  characters: string[];
}

export interface ServerConfig {
  address: string;
  port: number;
  encryption: EncryptionType;
  accounts: Account[];
  active_account_id: string | null;
  // 레거시 (마이그레이션 후엔 빈 문자열)
  username?: string;
  password_encrypted?: string;
}

export interface Profile {
  id: string;
  name: string;
  uo_path: string;
  cuo_path: string | null;
  server: ServerConfig;
  client_version: string | null;
}

export interface PluginEntry {
  path: string;
  enabled: boolean;
  /** 표시용 별명. 없으면 path 파일명 사용 */
  display_name?: string | null;
}

export interface UiSettings {
  language: string;
  /** 첫 사용 온보딩 배너 닫혔는지 (영구) */
  onboarding_dismissed?: boolean;
  /** PLAY/MULTI 한 번이라도 실행했는지 — 온보딩 3단계 자동 체크용 */
  first_launch_completed?: boolean;
  /** 설정 가이드를 한 번이라도 열었는지 (영구) — false면 사이드바 버튼 강조 */
  guide_opened?: boolean;
  /** 메인 화면에 노출할 프로필 id. 순서대로 최대 2개 */
  main_profile_ids?: string[] | null;
}

export interface Settings {
  active_profile_id: string | null;
  profiles: Profile[];
  plugins: PluginEntry[];
  ui: UiSettings;
}

// ── Notice ─────────────────────────────────────────────
export type Severity = "normal" | "urgent" | "event";

export interface Notice {
  id: string;
  title: string;
  date: string;
  severity: Severity;
  body_md: string;
  /** 선택: 전체 본문 외부 링크 (GitHub release/markdown 등) */
  url?: string | null;
  /** 선택: url 버튼에 표시할 짧은 라벨 */
  url_label?: string | null;
}

export interface NoticeBoard {
  margo: Notice[];
  ggouo: Notice[];
}

// ── Sidebar ─────────────────────────────────────────────
export interface SidebarLink {
  label: string;
  url?: string | null;
}

export interface SidebarGroup {
  label: string;
  buttons: SidebarLink[];
}

export interface Sidebar {
  groups: SidebarGroup[];
}

// ── Server Status (마고 서버 가용성) ────────────────────
export interface ServerEndpoint {
  host: string;
  port: number;
  label?: string;
}

export type ServerStatus =
  | { state: "online"; latency_ms: number }
  | { state: "offline"; reason: string };

// ── Paths ──────────────────────────────────────────────
export interface PathInfo {
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  valid_uo: boolean;
  valid_cuo: boolean;
}

/// detect_folder_kind 결과. Rust enum이 serde `tag = "kind"`로 직렬화됨.
export type FolderKind =
  | { kind: "new_install" }
  | { kind: "ggoce"; version: string }
  | { kind: "original_cuo" }
  | { kind: "unknown" };

// ── CUO Update ─────────────────────────────────────────
export interface ManifestFile {
  path: string;
  size: number;
  sha256: string;
}

export interface UpdateManifest {
  version: string;
  released: string;
  notes: string;
  base_url: string;
  files: ManifestFile[];
}

export type ChangeReason = "missing" | "size_mismatch" | "hash_mismatch";

export interface ChangedFile {
  path: string;
  size: number;
  reason: ChangeReason;
}

export interface UpdateCheck {
  remote_version: string;
  local_version: string | null;
  changed: ChangedFile[];
  total_bytes: number;
  manifest: UpdateManifest;
}

// ── Launcher Self Update ──────────────────────────────
export interface LauncherManifest {
  version: string;
  released: string;
  notes: string;
  url: string;
  size: number;
  sha256: string;
}

export interface SelfUpdateCheck {
  current_version: string;
  remote_version: string;
  update_available: boolean;
  manifest: LauncherManifest | null;
}
