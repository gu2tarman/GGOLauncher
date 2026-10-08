import type { EncryptionType, ServerConfig } from "./types";

export interface ServerPreset {
  id: string;
  label: string;
  address: string;
  port: number;
  encryption: EncryptionType;
}

/** 프로필 편집의 서버 선택 목록. 첫 항목이 새 프로필 기본값. */
export const SERVER_PRESETS: ServerPreset[] = [
  { id: "margo", label: "마고 (Margo)", address: "222.102.202.108", port: 2594, encryption: "auto" },
];

export const DEFAULT_SERVER_PRESET = SERVER_PRESETS[0];

export function findServerPreset(server: Pick<ServerConfig, "address" | "port">): ServerPreset | undefined {
  const address = server.address.trim().toLowerCase();
  return SERVER_PRESETS.find((p) => p.address === address && p.port === server.port);
}
