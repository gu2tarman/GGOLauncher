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
  /** MULTI LOGIN에서만 적용하는 보조 모니터 프리셋 슬롯. 미지정이면 창 관리 안 함. */
  secondary_slot?: "r0c0" | "r0c1" | "r1c0" | "r1c1" | "center" | null;
}

export type SecondaryLayoutPreset = "two_by_two" | "two_by_two_center";

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
  secondary_layout_preset: SecondaryLayoutPreset;
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
  /** 비상 격상: true면 버튼 펄스 강조 (원격 점등) */
  highlight?: boolean | null;
  /** 비상 격상: 버튼 우측 빨간 배지 텍스트 (예: "긴급") */
  badge?: string | null;
}

export interface SidebarGroup {
  label: string;
  buttons: SidebarLink[];
}

export interface Sidebar {
  groups: SidebarGroup[];
  /** 배경 아트 원격 교체용 HTTPS URL. 없으면 번들 기본 배경(bg-default.jpg) 사용 */
  background_url?: string | null;
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

// ── Stage 0 Multi-client Diagnostics ──────────────────
export interface MultiSessionStatus {
  selected_count: number;
  active_count: number;
  pending_count: number;
  untracked_count: number;
  missing_count: number;
}

export interface MultiLaunchResult {
  selected_count: number;
  launched_count: number;
  already_running_count: number;
  layout_warning: string | null;
}

export type GroupControlAction =
  | "minimize"
  | "restore_preset"
  | "group_raise"
  | "close_secondary";

export interface GroupControlResult {
  action: GroupControlAction;
  succeeded_count: number;
  pending_count: number;
  failed_count: number;
  accounts: Array<{
    account_id: string;
    slot: string;
    status: "success" | "pending" | "failed";
    message: string | null;
    window: {
      action: string;
      pid: number;
      hwnd: string;
      hwnd_generation: number;
      is_minimized: boolean | null;
      dpi: number | null;
    } | null;
  }>;
}

export interface Stage0SignedRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface Stage0GridCell {
  slot: string;
  rect: Stage0SignedRect;
}

export interface Stage0ActiveSession {
  runtime_session_id: number;
  profile_id: string;
  account_id: string;
  pid: number;
  process_creation_time_filetime_100ns: string | null;
  process_creation_time_error: string | null;
  launcher_observed_spawn_time_unix_ms: number;
}

export interface Stage0RecentExit extends Stage0ActiveSession {
  launcher_observed_exit_time_unix_ms: number;
  exit_code: number | null;
  success: boolean;
}

export interface Stage0RegistryWarning {
  code: string;
  profile_id: string | null;
  account_id: string | null;
  runtime_session_ids: number[];
  message: string;
}

export interface Stage0RegistrySnapshot {
  active_sessions: Stage0ActiveSession[];
  recent_exits: Stage0RecentExit[];
  untracked_launches: Array<{
    code: string;
    profile_id: string;
    account_id: string | null;
    launcher_observed_spawn_time_unix_ms: number;
  }>;
  warnings: Stage0RegistryWarning[];
}

export interface Stage0MonitorDryRun {
  monitor: {
    device_name: string;
    is_primary: boolean;
    monitor_rect: Stage0SignedRect;
    work_area: Stage0SignedRect;
  };
  cells: Stage0GridCell[] | null;
  layout_error: string | null;
}

export interface Stage1AWindowCandidate {
  hwnd: string;
  class_name: string;
  title: string;
  is_visible: boolean;
  has_owner: boolean;
  eligible_game_window: boolean;
  exclusion_reason: string | null;
  show_cmd: number | null;
  is_minimized: boolean | null;
  dpi: number | null;
  style: string;
  ex_style: string;
  window_rect: Stage0SignedRect | null;
  normal_rect: Stage0SignedRect | null;
  client_rect_screen: Stage0SignedRect | null;
}

export interface Stage1AProcessWindowInspection {
  pid: number;
  expected_process_creation_time_filetime_100ns: string | null;
  observed_process_creation_time_filetime_100ns: string | null;
  process_creation_time_matches: boolean | null;
  selected_hwnd: string | null;
  management_eligible: boolean;
  status: string;
  candidates: Stage1AWindowCandidate[];
  warnings: string[];
}

export interface Stage1ASessionWindow {
  runtime_session_id: number;
  profile_id: string;
  account_id: string;
  inspection: Stage1AProcessWindowInspection;
}

export interface Stage1BWindowActionResult {
  action: "move_test" | "position_only_test" | "restore_test";
  pid: number;
  hwnd: string;
  requested_outer_rect: Stage0SignedRect | null;
  actual_window_rect: Stage0SignedRect | null;
  actual_normal_rect: Stage0SignedRect | null;
  actual_client_rect_screen: Stage0SignedRect | null;
  requested_outer_rect_matches: boolean | null;
  requested_position_matches: boolean | null;
  show_cmd: number | null;
  is_minimized: boolean | null;
  dpi: number | null;
}

export interface Stage1BGroupWindowTestResult {
  action: "position_six_test" | "restore_all_test";
  windows: Stage1BSingleWindowTestResult[];
}

export interface Stage1BSingleWindowTestResult {
  runtime_session_id: number;
  monitor_device_name: string;
  slot: string;
  window: Stage1BWindowActionResult;
}

export interface Stage0Diagnostics {
  enabled: boolean;
  feature_flag: string;
  registry: Stage0RegistrySnapshot | null;
  registry_error: string | null;
  broker_sessions: Array<{
    launch_session_id: string;
    profile_id: string;
    account_id: string;
    expected_pid: number | null;
    connected: boolean;
    window_ready: boolean;
    managed_tile_active: boolean;
    hwnd: string | null;
    hwnd_generation: number | null;
    reconnect_count: number;
    pending_apply_layout: boolean;
    last_error: string | null;
  }>;
  session_windows: Stage1ASessionWindow[];
  window_test_restore_available_for: number[];
  monitors: Stage0MonitorDryRun[];
  monitor_warnings: string[];
  monitor_error: string | null;
}

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
