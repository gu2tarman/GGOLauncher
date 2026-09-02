mod broker;
mod layout;
mod session;
mod windows;

use crate::profile::SecondaryLayoutPreset;
pub(crate) use broker::{BrokerBootstrap, BrokerWindowTarget};
use layout::{split_2x2, split_2x2_with_center, GridCell};
use serde::Serialize;
use session::{
    AccountStateCounts, ActiveProcessIdentity, RegistrySnapshot, RuntimeSessionId, SessionRegistry,
};
use std::collections::HashMap;
use std::process::Child;
use std::sync::Mutex;
pub(crate) use windows::{
    close_broker_window, minimize_broker_window, raise_broker_window, restore_broker_window,
    WindowControlObservation,
};
use windows::{
    MonitorDescriptor, ProcessWindowInspection, SavedWindowPlacement, WindowActionResult,
};

pub const STAGE0_ENV_VAR: &str = "GGO_MULTICLIENT_STAGE0";

pub struct Stage0State {
    enabled: bool,
    broker: broker::BrokerServer,
    registry: Mutex<SessionRegistry>,
    window_test_placements: Mutex<HashMap<RuntimeSessionId, SavedWindowPlacement>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorDryRunDiagnostic {
    pub monitor: MonitorDescriptor,
    pub cells: Option<[GridCell; 4]>,
    pub layout_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionWindowDiagnostic {
    pub runtime_session_id: session::RuntimeSessionId,
    pub profile_id: String,
    pub account_id: String,
    pub inspection: ProcessWindowInspection,
}

#[derive(Debug, Clone, Serialize)]
pub struct SingleWindowTestResult {
    pub runtime_session_id: RuntimeSessionId,
    pub monitor_device_name: String,
    pub slot: &'static str,
    pub window: WindowActionResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupWindowTestResult {
    pub action: &'static str,
    pub windows: Vec<SingleWindowTestResult>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MultiSessionStatus {
    pub selected_count: usize,
    pub active_count: usize,
    pub pending_count: usize,
    pub untracked_count: usize,
    pub missing_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ManagedTile {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone)]
struct PositionOnlyTarget {
    monitor_device_name: String,
    slot: &'static str,
    left: i32,
    top: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stage0Diagnostics {
    pub enabled: bool,
    pub feature_flag: &'static str,
    pub registry: Option<RegistrySnapshot>,
    pub registry_error: Option<String>,
    pub broker_sessions: Vec<broker::BrokerSessionDiagnostic>,
    pub session_windows: Vec<SessionWindowDiagnostic>,
    pub window_test_restore_available_for: Vec<RuntimeSessionId>,
    pub monitors: Vec<MonitorDryRunDiagnostic>,
    pub monitor_warnings: Vec<String>,
    pub monitor_error: Option<String>,
}

impl Stage0State {
    pub fn from_environment() -> Self {
        Self::new(stage0_enabled_from_value(
            std::env::var(STAGE0_ENV_VAR).ok().as_deref(),
        ))
    }

    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            broker: broker::BrokerServer::start(),
            registry: Mutex::new(SessionRegistry::default()),
            window_test_placements: Mutex::new(HashMap::new()),
        }
    }

    /// Observe a successful normal Command::spawn without changing its result.
    /// If diagnostics cannot retain the Child, dropping Child keeps the process
    /// running and preserves the existing fail-open launch behavior.
    pub fn observe_spawn(
        &self,
        profile_id: &str,
        account_id: &str,
        child: Child,
        launcher_observed_spawn_time_unix_ms: u64,
    ) {
        match self.registry.lock() {
            Ok(mut registry) => {
                if let Err(error) = registry.register(
                    profile_id,
                    account_id,
                    child,
                    launcher_observed_spawn_time_unix_ms,
                ) {
                    eprintln!("[multiclient-stage0] registry registration failed: {error}");
                }
            }
            Err(_) => {
                eprintln!("[multiclient-stage0] registry lock poisoned; launch remains unmanaged");
            }
        }
    }

    pub fn prepare_broker_session(
        &self,
        profile_id: &str,
        account_id: &str,
        hud_leader: bool,
        hud_order: u8,
        managed_z_order: Option<u8>,
    ) -> Result<broker::BrokerBootstrap, String> {
        self.broker.prepare_session(
            profile_id,
            account_id,
            hud_leader,
            hud_order,
            managed_z_order,
        )
    }

    pub fn bind_broker_process(
        &self,
        bootstrap: &broker::BrokerBootstrap,
        pid: u32,
    ) -> Result<(), String> {
        let creation_time = windows::query_process_creation_time(pid)?;
        self.broker
            .bind_process(&bootstrap.launch_session_id, pid, Some(creation_time))
    }

    pub fn cancel_broker_session(&self, launch_session_id: &str) {
        self.broker.cancel_session(launch_session_id);
    }

    pub fn request_apply_managed_tile(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<(), String> {
        self.broker.request_apply_layout(profile_id, account_id)
    }

    pub fn broker_window_target(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<BrokerWindowTarget, String> {
        self.broker.window_target(profile_id, account_id)
    }

    pub fn disconnect_broker_session_for_test(
        &self,
        profile_id: &str,
        account_id: &str,
    ) -> Result<(), String> {
        self.require_enabled()?;
        self.broker
            .disconnect_session_for_test(profile_id, account_id)
    }

    pub fn observe_untracked_elevated_fallback(
        &self,
        profile_id: &str,
        account_id: Option<&str>,
        launcher_observed_spawn_time_unix_ms: u64,
    ) {
        match self.registry.lock() {
            Ok(mut registry) => registry.record_untracked_elevated_fallback(
                profile_id,
                account_id,
                launcher_observed_spawn_time_unix_ms,
            ),
            Err(_) => eprintln!(
                "[multiclient-stage0] registry lock poisoned; elevated fallback was not recorded"
            ),
        }
    }

    pub fn reserve_missing_accounts(
        &self,
        profile_id: &str,
        account_ids: &[String],
    ) -> Result<Vec<String>, String> {
        self.registry
            .lock()
            .map_err(|_| "multiclient session registry lock is poisoned".to_string())?
            .reserve_missing_accounts(profile_id, account_ids)
    }

    pub fn release_account_reservation(&self, profile_id: &str, account_id: &str) {
        match self.registry.lock() {
            Ok(mut registry) => registry.release_reservation(profile_id, account_id),
            Err(_) => eprintln!(
                "[multiclient] registry lock poisoned; account reservation was not released"
            ),
        }
    }

    pub fn multi_session_status(
        &self,
        profile_id: &str,
        account_ids: &[String],
    ) -> Result<MultiSessionStatus, String> {
        let counts = self
            .registry
            .lock()
            .map_err(|_| "multiclient session registry lock is poisoned".to_string())?
            .account_states(profile_id, account_ids);
        Ok(multi_session_status_from_counts(counts))
    }

    pub fn diagnostics(&self) -> Stage0Diagnostics {
        if !self.enabled {
            return Stage0Diagnostics {
                enabled: false,
                feature_flag: STAGE0_ENV_VAR,
                registry: None,
                registry_error: None,
                broker_sessions: Vec::new(),
                session_windows: Vec::new(),
                window_test_restore_available_for: Vec::new(),
                monitors: Vec::new(),
                monitor_warnings: Vec::new(),
                monitor_error: None,
            };
        }

        let (registry, registry_error) = match self.registry.lock() {
            Ok(mut registry) => (Some(registry.snapshot()), None),
            Err(_) => (
                None,
                Some("registry lock poisoned; active children were not inspected".to_string()),
            ),
        };

        let session_windows = registry
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .active_sessions
                    .iter()
                    .map(|session| {
                        let expected_creation_time = session
                            .process_creation_time_filetime_100ns
                            .as_deref()
                            .and_then(|value| value.parse::<u64>().ok());
                        SessionWindowDiagnostic {
                            runtime_session_id: session.runtime_session_id,
                            profile_id: session.profile_id.clone(),
                            account_id: session.account_id.clone(),
                            inspection: windows::inspect_process_windows(
                                session.pid,
                                expected_creation_time,
                            ),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let window_test_restore_available_for = self
            .window_test_placements
            .lock()
            .map(|placements| {
                let mut ids: Vec<RuntimeSessionId> = placements.keys().copied().collect();
                ids.sort();
                ids
            })
            .unwrap_or_default();

        let (monitors, monitor_warnings, monitor_error) = match windows::enumerate_monitors() {
            Ok(enumeration) => {
                let monitors = enumeration
                    .monitors
                    .into_iter()
                    .map(|monitor| match split_2x2(monitor.work_area) {
                        Ok(cells) => MonitorDryRunDiagnostic {
                            monitor,
                            cells: Some(cells),
                            layout_error: None,
                        },
                        Err(error) => MonitorDryRunDiagnostic {
                            monitor,
                            cells: None,
                            layout_error: Some(error.to_string()),
                        },
                    })
                    .collect();
                (monitors, enumeration.warnings, None)
            }
            Err(error) => (Vec::new(), Vec::new(), Some(error)),
        };

        Stage0Diagnostics {
            enabled: true,
            feature_flag: STAGE0_ENV_VAR,
            registry,
            registry_error,
            broker_sessions: self.broker.diagnostics(),
            session_windows,
            window_test_restore_available_for,
            monitors,
            monitor_warnings,
            monitor_error,
        }
    }

    pub fn move_single_window_test(
        &self,
        runtime_session_id: u64,
        slot: &str,
    ) -> Result<SingleWindowTestResult, String> {
        self.require_enabled()?;
        let runtime_session_id = RuntimeSessionId(runtime_session_id);
        let identity = self.active_process_identity(runtime_session_id)?;
        let creation_time = identity
            .process_creation_time_filetime_100ns
            .ok_or_else(|| {
                format!(
                    "runtime session #{} has no verified process creation time",
                    runtime_session_id.0
                )
            })?;
        let (monitor, cell) = secondary_test_cell(slot)?;

        let existing_snapshot = self
            .window_test_placements
            .lock()
            .map_err(|_| "window test placement lock poisoned".to_string())?
            .get(&runtime_session_id)
            .cloned();
        let is_new_snapshot = existing_snapshot.is_none();
        let snapshot = match existing_snapshot {
            Some(snapshot) => {
                if snapshot.pid != identity.pid
                    || snapshot.process_creation_time_filetime_100ns != creation_time
                {
                    return Err(format!(
                        "saved placement identity no longer matches runtime session #{}",
                        runtime_session_id.0
                    ));
                }
                snapshot
            }
            None => windows::capture_game_window(identity.pid, creation_time)?,
        };

        if is_new_snapshot {
            self.window_test_placements
                .lock()
                .map_err(|_| "window test placement lock poisoned".to_string())?
                .insert(runtime_session_id, snapshot.clone());
        }

        match windows::apply_test_outer_rect(&snapshot, cell.rect) {
            Ok(window) => Ok(SingleWindowTestResult {
                runtime_session_id,
                monitor_device_name: monitor.device_name,
                slot: cell.slot,
                window,
            }),
            Err(error) => {
                if is_new_snapshot {
                    let _ = windows::restore_saved_window(&snapshot);
                    if let Ok(mut placements) = self.window_test_placements.lock() {
                        placements.remove(&runtime_session_id);
                    }
                }
                Err(error)
            }
        }
    }

    pub fn restore_single_window_test(
        &self,
        runtime_session_id: u64,
    ) -> Result<SingleWindowTestResult, String> {
        self.require_enabled()?;
        let runtime_session_id = RuntimeSessionId(runtime_session_id);
        let identity = self.active_process_identity(runtime_session_id)?;
        let snapshot = self
            .window_test_placements
            .lock()
            .map_err(|_| "window test placement lock poisoned".to_string())?
            .get(&runtime_session_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "runtime session #{} has no saved test placement",
                    runtime_session_id.0
                )
            })?;
        if snapshot.pid != identity.pid
            || Some(snapshot.process_creation_time_filetime_100ns)
                != identity.process_creation_time_filetime_100ns
        {
            return Err(format!(
                "saved placement identity no longer matches runtime session #{}",
                runtime_session_id.0
            ));
        }

        let window = windows::restore_saved_window(&snapshot)?;
        self.window_test_placements
            .lock()
            .map_err(|_| "window test placement lock poisoned".to_string())?
            .remove(&runtime_session_id);
        Ok(SingleWindowTestResult {
            runtime_session_id,
            monitor_device_name: "original".to_string(),
            slot: "original",
            window,
        })
    }

    pub fn position_six_windows_test(&self) -> Result<GroupWindowTestResult, String> {
        self.require_enabled()?;
        let active_sessions = self
            .registry
            .lock()
            .map_err(|_| "registry lock poisoned".to_string())?
            .snapshot()
            .active_sessions;
        if active_sessions.len() != 6 {
            return Err(format!(
                "six-window position test requires exactly 6 active sessions; found {}",
                active_sessions.len()
            ));
        }

        let runtime_session_ids: Vec<RuntimeSessionId> = active_sessions
            .iter()
            .map(|session| session.runtime_session_id)
            .collect();
        {
            let placements = self
                .window_test_placements
                .lock()
                .map_err(|_| "window test placement lock poisoned".to_string())?;
            if let Some(existing) = runtime_session_ids
                .iter()
                .find(|runtime_session_id| placements.contains_key(runtime_session_id))
            {
                return Err(format!(
                    "runtime session #{} already has a saved test placement; restore it first",
                    existing.0
                ));
            }
        }

        let targets = six_position_only_targets()?;
        let mut captured = Vec::with_capacity(6);
        for session in active_sessions {
            let identity = self.active_process_identity(session.runtime_session_id)?;
            let creation_time = identity
                .process_creation_time_filetime_100ns
                .ok_or_else(|| {
                    format!(
                        "runtime session #{} has no verified process creation time",
                        session.runtime_session_id.0
                    )
                })?;
            let snapshot = windows::capture_game_window(identity.pid, creation_time)?;
            captured.push((session.runtime_session_id, snapshot));
        }

        {
            let mut placements = self
                .window_test_placements
                .lock()
                .map_err(|_| "window test placement lock poisoned".to_string())?;
            for (runtime_session_id, snapshot) in &captured {
                placements.insert(*runtime_session_id, snapshot.clone());
            }
        }

        let mut results = Vec::with_capacity(6);
        for ((runtime_session_id, snapshot), target) in captured.iter().zip(&targets) {
            match windows::apply_test_position_only(snapshot, target.left, target.top) {
                Ok(window) => results.push(SingleWindowTestResult {
                    runtime_session_id: *runtime_session_id,
                    monitor_device_name: target.monitor_device_name.clone(),
                    slot: target.slot,
                    window,
                }),
                Err(error) => {
                    let mut rollback_errors = Vec::new();
                    for (rollback_session_id, rollback_snapshot) in captured.iter().rev() {
                        if let Err(rollback_error) =
                            windows::restore_saved_window(rollback_snapshot)
                        {
                            rollback_errors
                                .push(format!("#{}: {rollback_error}", rollback_session_id.0));
                        }
                    }
                    if let Ok(mut placements) = self.window_test_placements.lock() {
                        for runtime_session_id in &runtime_session_ids {
                            placements.remove(runtime_session_id);
                        }
                    }
                    return if rollback_errors.is_empty() {
                        Err(format!(
                            "six-window position test failed and was rolled back: {error}"
                        ))
                    } else {
                        Err(format!(
                            "six-window position test failed: {error}; rollback errors: {}",
                            rollback_errors.join(" | ")
                        ))
                    };
                }
            }
        }

        Ok(GroupWindowTestResult {
            action: "position_six_test",
            windows: results,
        })
    }

    pub fn restore_all_window_tests(&self) -> Result<GroupWindowTestResult, String> {
        self.require_enabled()?;
        let runtime_session_ids = self
            .window_test_placements
            .lock()
            .map_err(|_| "window test placement lock poisoned".to_string())?
            .keys()
            .copied()
            .collect::<Vec<_>>();
        if runtime_session_ids.is_empty() {
            return Err("there are no saved test placements to restore".to_string());
        }

        let mut results = Vec::with_capacity(runtime_session_ids.len());
        let mut errors = Vec::new();
        for runtime_session_id in runtime_session_ids {
            match self.restore_single_window_test(runtime_session_id.0) {
                Ok(result) => results.push(result),
                Err(error) => errors.push(format!("#{}: {error}", runtime_session_id.0)),
            }
        }
        if !errors.is_empty() {
            return Err(format!(
                "some windows could not be restored: {}",
                errors.join(" | ")
            ));
        }

        Ok(GroupWindowTestResult {
            action: "restore_all_test",
            windows: results,
        })
    }

    fn active_process_identity(
        &self,
        runtime_session_id: RuntimeSessionId,
    ) -> Result<ActiveProcessIdentity, String> {
        self.registry
            .lock()
            .map_err(|_| "registry lock poisoned".to_string())?
            .active_process_identity(runtime_session_id)
    }

    fn require_enabled(&self) -> Result<(), String> {
        if self.enabled {
            Ok(())
        } else {
            Err(format!("{} is not enabled", STAGE0_ENV_VAR))
        }
    }
}

fn multi_session_status_from_counts(counts: AccountStateCounts) -> MultiSessionStatus {
    MultiSessionStatus {
        selected_count: counts.selected,
        active_count: counts.active,
        pending_count: counts.pending,
        untracked_count: counts.untracked,
        missing_count: counts.missing,
    }
}

fn secondary_test_cell(slot: &str) -> Result<(MonitorDescriptor, GridCell), String> {
    let enumeration = windows::enumerate_monitors()?;
    let monitor = enumeration
        .monitors
        .into_iter()
        .filter(|monitor| !monitor.is_primary)
        .max_by(|left, right| {
            let left_area = left.work_area.width() * left.work_area.height();
            let right_area = right.work_area.width() * right.work_area.height();
            left_area
                .cmp(&right_area)
                .then_with(|| right.device_name.cmp(&left.device_name))
        })
        .ok_or_else(|| "no non-primary monitor is available for the test".to_string())?;
    let cells = split_2x2(monitor.work_area).map_err(|error| error.to_string())?;
    let cell = cells
        .into_iter()
        .find(|cell| cell.slot == slot)
        .ok_or_else(|| format!("unknown 2x2 test slot: {slot}"))?;
    Ok((monitor, cell))
}

pub(crate) fn secondary_managed_tiles(
    preset: SecondaryLayoutPreset,
) -> Result<Option<HashMap<&'static str, ManagedTile>>, String> {
    let enumeration = windows::enumerate_monitors()?;
    let Some(monitor) = enumeration
        .monitors
        .into_iter()
        .filter(|monitor| !monitor.is_primary)
        .max_by(|left, right| {
            let left_area = left.work_area.width() * left.work_area.height();
            let right_area = right.work_area.width() * right.work_area.height();
            left_area
                .cmp(&right_area)
                .then_with(|| right.device_name.cmp(&left.device_name))
        })
    else {
        return Ok(None);
    };

    let cells = match preset {
        SecondaryLayoutPreset::TwoByTwo => {
            split_2x2(monitor.work_area).map(|cells| cells.into_iter().collect::<Vec<_>>())
        }
        SecondaryLayoutPreset::TwoByTwoCenter => split_2x2_with_center(monitor.work_area)
            .map(|cells| cells.into_iter().collect::<Vec<_>>()),
    }
    .map_err(|error| error.to_string())?;
    let mut tiles = HashMap::with_capacity(cells.len());
    for cell in cells {
        let width = i32::try_from(cell.rect.width())
            .map_err(|_| "secondary tile width exceeds the supported range".to_string())?;
        let height = i32::try_from(cell.rect.height())
            .map_err(|_| "secondary tile height exceeds the supported range".to_string())?;
        tiles.insert(
            cell.slot,
            ManagedTile {
                left: cell.rect.left,
                top: cell.rect.top,
                width,
                height,
            },
        );
    }
    Ok(Some(tiles))
}

fn six_position_only_targets() -> Result<Vec<PositionOnlyTarget>, String> {
    let enumeration = windows::enumerate_monitors()?;
    let primary = enumeration
        .monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .cloned()
        .ok_or_else(|| "no primary monitor is available for the test".to_string())?;
    let secondary = enumeration
        .monitors
        .into_iter()
        .filter(|monitor| !monitor.is_primary)
        .max_by(|left, right| {
            let left_area = left.work_area.width() * left.work_area.height();
            let right_area = right.work_area.width() * right.work_area.height();
            left_area
                .cmp(&right_area)
                .then_with(|| right.device_name.cmp(&left.device_name))
        })
        .ok_or_else(|| "no non-primary monitor is available for the test".to_string())?;
    let primary_cells = split_2x2(primary.work_area).map_err(|error| error.to_string())?;
    let secondary_cells = split_2x2(secondary.work_area).map_err(|error| error.to_string())?;

    let mut targets = Vec::with_capacity(6);
    for cell in primary_cells.into_iter().take(2) {
        targets.push(PositionOnlyTarget {
            monitor_device_name: primary.device_name.clone(),
            slot: cell.slot,
            left: cell.rect.left,
            top: cell.rect.top,
        });
    }
    for cell in secondary_cells {
        targets.push(PositionOnlyTarget {
            monitor_device_name: secondary.device_name.clone(),
            slot: cell.slot,
            left: cell.rect.left,
            top: cell.rect.top,
        });
    }
    Ok(targets)
}

fn stage0_enabled_from_value(value: Option<&str>) -> bool {
    value == Some("1")
}

pub fn launcher_observed_time_ms() -> u64 {
    session::unix_time_ms()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn feature_flag_requires_exact_value_one() {
        assert!(stage0_enabled_from_value(Some("1")));
        assert!(!stage0_enabled_from_value(None));
        assert!(!stage0_enabled_from_value(Some("0")));
        assert!(!stage0_enabled_from_value(Some("true")));
        assert!(!stage0_enabled_from_value(Some(" 1")));
    }

    #[test]
    fn disabled_diagnostics_do_not_enumerate_or_expose_runtime_state() {
        let state = Stage0State::new(false);
        let diagnostics = state.diagnostics();

        assert!(!diagnostics.enabled);
        assert!(diagnostics.registry.is_none());
        assert!(diagnostics.session_windows.is_empty());
        assert!(diagnostics.monitors.is_empty());
        assert!(diagnostics.monitor_error.is_none());
        assert!(state.move_single_window_test(1, "r0c0").is_err());
        assert!(state.restore_single_window_test(1).is_err());
        assert!(state.position_six_windows_test().is_err());
        assert!(state.restore_all_window_tests().is_err());
    }

    #[cfg(windows)]
    #[test]
    fn enabled_state_records_normal_spawn_identity_and_builds_monitor_dry_runs() {
        let state = Stage0State::new(true);
        let child = Command::new("cmd")
            .args(["/C", "ping -n 4 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        let pid = child.id();
        state.observe_spawn("profile-a", "account-a", child, launcher_observed_time_ms());

        let diagnostics = state.diagnostics();
        let registry = diagnostics.registry.as_ref().unwrap();
        assert_eq!(registry.active_sessions.len(), 1);
        assert_eq!(registry.active_sessions[0].profile_id, "profile-a");
        assert_eq!(registry.active_sessions[0].account_id, "account-a");
        assert_eq!(registry.active_sessions[0].pid, pid);
        assert!(registry.active_sessions[0]
            .process_creation_time_filetime_100ns
            .is_some());
        assert_eq!(diagnostics.session_windows.len(), 1);
        assert_eq!(
            diagnostics.session_windows[0]
                .inspection
                .process_creation_time_matches,
            Some(true)
        );
        assert!(!diagnostics.monitors.is_empty());
        assert!(diagnostics
            .monitors
            .iter()
            .all(|monitor| monitor.cells.is_some() && monitor.layout_error.is_none()));

        eprintln!(
            "STAGE0_FULL_DIAGNOSTICS={}",
            serde_json::to_string_pretty(&diagnostics).unwrap()
        );

        state.registry.lock().unwrap().kill_all_for_test();
    }
}
