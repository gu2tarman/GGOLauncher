// Rust 측 모델과 1:1 미러. 변경 시 양쪽 같이 수정.

export type EncryptionType =
  | "auto"
  | "none"
  | "old_blowfish"
  | "blowfish125"
  | "twofish";

export interface ServerConfig {
  address: string;
  port: number;
  username: string;
  password_encrypted: string;
  encryption: EncryptionType;
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
