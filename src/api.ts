import { invoke } from "@tauri-apps/api/core";
import type {
  FolderKind,
  LauncherManifest,
  NoticeBoard,
  PathInfo,
  SelfUpdateCheck,
  Settings,
  UpdateCheck,
} from "./types";

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
  detectFolderKind: (path: string) => invoke<FolderKind>("detect_folder_kind", { path }),
  getLauncherDir: () => invoke<string | null>("get_launcher_dir"),
  clientSelectDirectory: (startDir?: string) =>
    invoke<string | null>("client_select_directory", { startDir: startDir ?? null }),
  cuoSelectDirectory: (startDir?: string) =>
    invoke<string | null>("cuo_select_directory", { startDir: startDir ?? null }),

  pluginSelectFile: () => invoke<string | null>("plugin_select_file"),
  importPluginFromZip: () => invoke<string | null>("import_plugin_from_zip"),

  cuoLaunch: (profileId: string, accountId?: string | null) =>
    invoke<void>("cuo_launch", { profileId, accountId: accountId ?? null }),
  cuoLaunchMulti: (profileId: string, delayMs?: number) =>
    invoke<number>("cuo_launch_multi", { profileId, delayMs: delayMs ?? null }),

  encryptPassword: (plain: string) => invoke<string>("encrypt_password", { plain }),
  decryptPassword: (stored: string) => invoke<string>("decrypt_password", { stored }),

  cuoCheckUpdate: (cuoPath: string) => invoke<UpdateCheck>("cuo_check_update", { cuoPath }),
  cuoFetchManifestForInstall: () =>
    invoke<UpdateCheck>("cuo_fetch_manifest_for_install"),
  cuoApplyUpdate: (
    cuoPath: string,
    check: UpdateCheck,
    allowOriginalOverwrite: boolean,
  ) =>
    invoke<void>("cuo_apply_update", {
      cuoPath,
      check,
      allowOriginalOverwrite,
    }),

  launcherCheckUpdate: () => invoke<SelfUpdateCheck>("launcher_check_update"),
  launcherApplyUpdate: (manifest: LauncherManifest) =>
    invoke<void>("launcher_apply_update", { manifest }),
  quitLauncher: () => invoke<void>("quit_launcher"),
};
