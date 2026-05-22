import { invoke } from "@tauri-apps/api/core";
import type { NoticeBoard, PathInfo, Settings } from "./types";

export const api = {
  openExternal: (url: string) => invoke<void>("open_external", { url }),

  launcherInit: () => invoke<Settings>("launcher_init"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  fetchNotice: () => invoke<NoticeBoard>("fetch_notice"),

  inspectPath: (path: string) => invoke<PathInfo>("inspect_path", { path }),
  detectClientVersion: (uoPath: string) =>
    invoke<string | null>("detect_client_version", { uoPath }),
  detectGgoceVersion: (cuoPath: string) =>
    invoke<string | null>("detect_ggoce_version", { cuoPath }),
  clientSelectDirectory: (startDir?: string) =>
    invoke<string | null>("client_select_directory", { startDir: startDir ?? null }),
  cuoSelectDirectory: (startDir?: string) =>
    invoke<string | null>("cuo_select_directory", { startDir: startDir ?? null }),

  pluginSelectFile: () => invoke<string | null>("plugin_select_file"),
  importPluginFromZip: () => invoke<string | null>("import_plugin_from_zip"),
};
