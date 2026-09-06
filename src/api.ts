import { invoke } from "@tauri-apps/api/core";
import type {
  CuoProfileCandidate,
  FolderKind,
  GroupControlAction,
  GroupControlResult,
  LauncherManifest,
  MultiLaunchResult,
  MultiSessionStatus,
  NoticeBoard,
  PathInfo,
  SelfUpdateCheck,
  Settings,
  Sidebar,
  Stage0Diagnostics,
  Stage1BGroupWindowTestResult,
  Stage1BSingleWindowTestResult,
  UpdateCheck,
} from "./types";

export const api = {
  openExternal: (url: string) => invoke<void>("open_external", { url }),

  launcherInit: () => invoke<Settings>("launcher_init"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),

  fetchNotice: () => invoke<NoticeBoard>("fetch_notice"),
  fetchSidebar: () => invoke<Sidebar>("fetch_sidebar"),
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
  listCuoProfiles: (cuoPath: string) =>
    invoke<CuoProfileCandidate[]>("list_cuo_profiles", { cuoPath }),

  addPlugin: () => invoke<string | null>("add_plugin"),

  cuoLaunch: (profileId: string, accountId?: string | null) =>
    invoke<void>("cuo_launch", { profileId, accountId: accountId ?? null }),
  cuoLaunchMulti: (profileId: string, delayMs?: number) =>
    invoke<MultiLaunchResult>("cuo_launch_multi", { profileId, delayMs: delayMs ?? null }),
  multiclientSessionStatus: (profileId: string) =>
    invoke<MultiSessionStatus>("multiclient_session_status", { profileId }),
  multiclientGroupControl: (profileId: string, action: GroupControlAction) =>
    invoke<GroupControlResult>("multiclient_group_control", { profileId, action }),
  multiclientStage0Diagnostics: () =>
    invoke<Stage0Diagnostics>("multiclient_stage0_diagnostics"),
  multiclientStage0DisconnectBrokerTest: (profileId: string, accountId: string) =>
    invoke<void>("multiclient_stage0_disconnect_broker_test", {
      profileId,
      accountId,
    }),
  multiclientStage1BMoveTest: (runtimeSessionId: number, slot = "r0c0") =>
    invoke<Stage1BSingleWindowTestResult>("multiclient_stage1b_move_test", {
      runtimeSessionId,
      slot,
    }),
  multiclientStage1BRestoreTest: (runtimeSessionId: number) =>
    invoke<Stage1BSingleWindowTestResult>("multiclient_stage1b_restore_test", {
      runtimeSessionId,
    }),
  multiclientStage1BPositionSixTest: () =>
    invoke<Stage1BGroupWindowTestResult>("multiclient_stage1b_position_six_test"),
  multiclientStage1BRestoreAllTest: () =>
    invoke<Stage1BGroupWindowTestResult>("multiclient_stage1b_restore_all_test"),

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
