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
  display_name?: string | null;
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
}

export interface UiSettings {
  language: string;
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
}

export interface NoticeBoard {
  margo: Notice[];
  ggouo: Notice[];
}

// ── Paths ──────────────────────────────────────────────
export interface PathInfo {
  exists: boolean;
  is_dir: boolean;
  is_file: boolean;
  valid_uo: boolean;
  valid_cuo: boolean;
}
