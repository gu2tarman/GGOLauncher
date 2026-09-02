use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::process::Child;
use std::time::{SystemTime, UNIX_EPOCH};

const RECENT_EXIT_CAPACITY: usize = 32;
const RECENT_UNTRACKED_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RuntimeSessionId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AccountKey {
    profile_id: String,
    account_id: String,
}

struct SessionEntry {
    runtime_session_id: RuntimeSessionId,
    profile_id: String,
    account_id: String,
    pid: u32,
    process_creation_time_filetime_100ns: Option<u64>,
    process_creation_time_error: Option<String>,
    launcher_observed_spawn_time_unix_ms: u64,
    child: Child,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveSessionDiagnostic {
    pub runtime_session_id: RuntimeSessionId,
    pub profile_id: String,
    pub account_id: String,
    pub pid: u32,
    /// Decimal FILETIME ticks, serialized as text to avoid JavaScript u64 precision loss.
    pub process_creation_time_filetime_100ns: Option<String>,
    pub process_creation_time_error: Option<String>,
    pub launcher_observed_spawn_time_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentExitDiagnostic {
    pub runtime_session_id: RuntimeSessionId,
    pub profile_id: String,
    pub account_id: String,
    pub pid: u32,
    pub process_creation_time_filetime_100ns: Option<String>,
    pub process_creation_time_error: Option<String>,
    pub launcher_observed_spawn_time_unix_ms: u64,
    pub launcher_observed_exit_time_unix_ms: u64,
    pub exit_code: Option<i32>,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UntrackedLaunchDiagnostic {
    pub code: &'static str,
    pub profile_id: String,
    pub account_id: Option<String>,
    pub launcher_observed_spawn_time_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistryWarning {
    pub code: &'static str,
    pub profile_id: Option<String>,
    pub account_id: Option<String>,
    pub runtime_session_ids: Vec<RuntimeSessionId>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegistrySnapshot {
    pub active_sessions: Vec<ActiveSessionDiagnostic>,
    pub recent_exits: Vec<RecentExitDiagnostic>,
    pub untracked_launches: Vec<UntrackedLaunchDiagnostic>,
    pub warnings: Vec<RegistryWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveProcessIdentity {
    pub runtime_session_id: RuntimeSessionId,
    pub pid: u32,
    pub process_creation_time_filetime_100ns: Option<u64>,
}

pub struct SessionRegistry {
    next_runtime_session_id: u64,
    active: BTreeMap<RuntimeSessionId, SessionEntry>,
    by_account: HashMap<AccountKey, Vec<RuntimeSessionId>>,
    pending_accounts: HashSet<AccountKey>,
    untracked_active_accounts: HashSet<AccountKey>,
    recent_exits: VecDeque<RecentExitDiagnostic>,
    untracked_launches: VecDeque<UntrackedLaunchDiagnostic>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self {
            next_runtime_session_id: 1,
            active: BTreeMap::new(),
            by_account: HashMap::new(),
            pending_accounts: HashSet::new(),
            untracked_active_accounts: HashSet::new(),
            recent_exits: VecDeque::with_capacity(RECENT_EXIT_CAPACITY),
            untracked_launches: VecDeque::with_capacity(RECENT_UNTRACKED_CAPACITY),
        }
    }
}

impl SessionRegistry {
    pub fn reserve_missing_accounts(
        &mut self,
        profile_id: &str,
        account_ids: &[String],
    ) -> Result<Vec<String>, String> {
        let _ = self.reap_exited();
        let mut seen = HashSet::new();
        let mut reserved = Vec::new();

        for account_id in account_ids {
            if !seen.insert(account_id.as_str()) {
                return Err(format!(
                    "duplicate account id in MULTI LOGIN selection: {account_id}"
                ));
            }

            let key = AccountKey {
                profile_id: profile_id.to_string(),
                account_id: account_id.clone(),
            };
            let is_active = self
                .by_account
                .get(&key)
                .is_some_and(|sessions| !sessions.is_empty());
            if is_active
                || self.pending_accounts.contains(&key)
                || self.untracked_active_accounts.contains(&key)
            {
                continue;
            }

            self.pending_accounts.insert(key);
            reserved.push(account_id.clone());
        }

        Ok(reserved)
    }

    pub fn release_reservation(&mut self, profile_id: &str, account_id: &str) {
        self.pending_accounts.remove(&AccountKey {
            profile_id: profile_id.to_string(),
            account_id: account_id.to_string(),
        });
    }

    pub fn account_states(
        &mut self,
        profile_id: &str,
        account_ids: &[String],
    ) -> AccountStateCounts {
        let _ = self.reap_exited();
        let mut counts = AccountStateCounts::default();
        let mut seen = HashSet::new();

        for account_id in account_ids {
            if !seen.insert(account_id.as_str()) {
                continue;
            }
            counts.selected += 1;
            let key = AccountKey {
                profile_id: profile_id.to_string(),
                account_id: account_id.clone(),
            };
            if self
                .by_account
                .get(&key)
                .is_some_and(|sessions| !sessions.is_empty())
            {
                counts.active += 1;
            } else if self.pending_accounts.contains(&key) {
                counts.pending += 1;
            } else if self.untracked_active_accounts.contains(&key) {
                counts.untracked += 1;
            } else {
                counts.missing += 1;
            }
        }

        counts
    }

    pub fn active_process_identity(
        &mut self,
        runtime_session_id: RuntimeSessionId,
    ) -> Result<ActiveProcessIdentity, String> {
        let _ = self.reap_exited();
        let entry = self
            .active
            .get(&runtime_session_id)
            .ok_or_else(|| format!("runtime session #{} is not active", runtime_session_id.0))?;
        Ok(ActiveProcessIdentity {
            runtime_session_id,
            pid: entry.pid,
            process_creation_time_filetime_100ns: entry.process_creation_time_filetime_100ns,
        })
    }

    pub fn register(
        &mut self,
        profile_id: &str,
        account_id: &str,
        child: Child,
        launcher_observed_spawn_time_unix_ms: u64,
    ) -> Result<RuntimeSessionId, String> {
        let runtime_session_id = RuntimeSessionId(self.next_runtime_session_id);
        self.next_runtime_session_id = self
            .next_runtime_session_id
            .checked_add(1)
            .ok_or_else(|| "runtime session id counter exhausted".to_string())?;

        let pid = child.id();
        let (process_creation_time_filetime_100ns, process_creation_time_error) =
            match super::windows::query_process_creation_time(pid) {
                Ok(value) => (Some(value), None),
                Err(error) => (None, Some(error)),
            };
        let key = AccountKey {
            profile_id: profile_id.to_string(),
            account_id: account_id.to_string(),
        };
        self.pending_accounts.remove(&key);
        self.by_account
            .entry(key)
            .or_default()
            .push(runtime_session_id);
        self.active.insert(
            runtime_session_id,
            SessionEntry {
                runtime_session_id,
                profile_id: profile_id.to_string(),
                account_id: account_id.to_string(),
                pid,
                process_creation_time_filetime_100ns,
                process_creation_time_error,
                launcher_observed_spawn_time_unix_ms,
                child,
            },
        );
        Ok(runtime_session_id)
    }

    pub fn record_untracked_elevated_fallback(
        &mut self,
        profile_id: &str,
        account_id: Option<&str>,
        launcher_observed_spawn_time_unix_ms: u64,
    ) {
        if let Some(account_id) = account_id {
            let key = AccountKey {
                profile_id: profile_id.to_string(),
                account_id: account_id.to_string(),
            };
            self.pending_accounts.remove(&key);
            // ShellExecute elevation fallback does not return a Child handle in
            // the current launcher. Keep it occupied for this launcher run so a
            // second MULTI click cannot create an obvious duplicate client.
            self.untracked_active_accounts.insert(key);
        }
        push_bounded(
            &mut self.untracked_launches,
            RECENT_UNTRACKED_CAPACITY,
            UntrackedLaunchDiagnostic {
                code: "untracked_elevated_fallback",
                profile_id: profile_id.to_string(),
                account_id: account_id.map(str::to_string),
                launcher_observed_spawn_time_unix_ms,
            },
        );
    }

    pub fn snapshot(&mut self) -> RegistrySnapshot {
        let mut warnings = self.reap_exited();

        for (key, runtime_session_ids) in &self.by_account {
            if runtime_session_ids.len() > 1 {
                warnings.push(RegistryWarning {
                    code: "duplicate_account",
                    profile_id: Some(key.profile_id.clone()),
                    account_id: Some(key.account_id.clone()),
                    runtime_session_ids: runtime_session_ids.clone(),
                    message: format!(
                        "{} active sessions share the same profile_id/account_id",
                        runtime_session_ids.len()
                    ),
                });
            }
        }

        RegistrySnapshot {
            active_sessions: self
                .active
                .values()
                .map(|entry| ActiveSessionDiagnostic {
                    runtime_session_id: entry.runtime_session_id,
                    profile_id: entry.profile_id.clone(),
                    account_id: entry.account_id.clone(),
                    pid: entry.pid,
                    process_creation_time_filetime_100ns: entry
                        .process_creation_time_filetime_100ns
                        .map(|value| value.to_string()),
                    process_creation_time_error: entry.process_creation_time_error.clone(),
                    launcher_observed_spawn_time_unix_ms: entry
                        .launcher_observed_spawn_time_unix_ms,
                })
                .collect(),
            recent_exits: self.recent_exits.iter().cloned().collect(),
            untracked_launches: self.untracked_launches.iter().cloned().collect(),
            warnings,
        }
    }

    fn reap_exited(&mut self) -> Vec<RegistryWarning> {
        let mut exited = Vec::new();
        let mut warnings = Vec::new();

        for (runtime_session_id, entry) in &mut self.active {
            match entry.child.try_wait() {
                Ok(Some(status)) => exited.push((*runtime_session_id, status)),
                Ok(None) => {}
                Err(error) => warnings.push(RegistryWarning {
                    code: "try_wait_failed",
                    profile_id: Some(entry.profile_id.clone()),
                    account_id: Some(entry.account_id.clone()),
                    runtime_session_ids: vec![*runtime_session_id],
                    message: error.to_string(),
                }),
            }
        }

        for (runtime_session_id, status) in exited {
            let Some(entry) = self.active.remove(&runtime_session_id) else {
                continue;
            };
            let key = AccountKey {
                profile_id: entry.profile_id.clone(),
                account_id: entry.account_id.clone(),
            };
            if let Some(ids) = self.by_account.get_mut(&key) {
                ids.retain(|id| *id != runtime_session_id);
                if ids.is_empty() {
                    self.by_account.remove(&key);
                }
            }

            push_bounded(
                &mut self.recent_exits,
                RECENT_EXIT_CAPACITY,
                RecentExitDiagnostic {
                    runtime_session_id,
                    profile_id: entry.profile_id,
                    account_id: entry.account_id,
                    pid: entry.pid,
                    process_creation_time_filetime_100ns: entry
                        .process_creation_time_filetime_100ns
                        .map(|value| value.to_string()),
                    process_creation_time_error: entry.process_creation_time_error,
                    launcher_observed_spawn_time_unix_ms: entry
                        .launcher_observed_spawn_time_unix_ms,
                    launcher_observed_exit_time_unix_ms: unix_time_ms(),
                    exit_code: status.code(),
                    success: status.success(),
                },
            );
        }

        warnings
    }

    #[cfg(test)]
    pub(super) fn kill_all_for_test(&mut self) {
        for entry in self.active.values_mut() {
            let _ = entry.child.kill();
            let _ = entry.child.wait();
        }
        self.active.clear();
        self.by_account.clear();
        self.pending_accounts.clear();
        self.untracked_active_accounts.clear();
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccountStateCounts {
    pub selected: usize,
    pub active: usize,
    pub pending: usize,
    pub untracked: usize,
    pub missing: usize,
}

pub fn unix_time_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn push_bounded<T>(queue: &mut VecDeque<T>, capacity: usize, value: T) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    #[cfg(windows)]
    fn spawn_sleeper() -> Child {
        Command::new("cmd")
            .args(["/C", "ping -n 4 127.0.0.1 >NUL"])
            .spawn()
            .unwrap()
    }

    #[cfg(not(windows))]
    fn spawn_sleeper() -> Child {
        Command::new("sh").args(["-c", "sleep 3"]).spawn().unwrap()
    }

    #[cfg(windows)]
    fn spawn_with_exit_code(code: i32) -> Child {
        Command::new("cmd")
            .args(["/C", &format!("exit {code}")])
            .spawn()
            .unwrap()
    }

    #[cfg(not(windows))]
    fn spawn_with_exit_code(code: i32) -> Child {
        Command::new("sh")
            .args(["-c", &format!("exit {code}")])
            .spawn()
            .unwrap()
    }

    #[test]
    fn duplicate_accounts_keep_distinct_runtime_sessions() {
        let mut registry = SessionRegistry::default();
        let first = registry
            .register("profile-a", "account-a", spawn_sleeper(), unix_time_ms())
            .unwrap();
        let second = registry
            .register("profile-a", "account-a", spawn_sleeper(), unix_time_ms())
            .unwrap();

        let snapshot = registry.snapshot();
        assert_ne!(first, second);
        assert_eq!(snapshot.active_sessions.len(), 2);
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "duplicate_account"
                && warning.runtime_session_ids == vec![first, second]));

        registry.kill_all_for_test();
    }

    #[test]
    fn reservations_make_repeated_multi_plans_idempotent() {
        let mut registry = SessionRegistry::default();
        let accounts = vec!["account-a".to_string(), "account-b".to_string()];

        assert_eq!(
            registry
                .reserve_missing_accounts("profile-a", &accounts)
                .unwrap(),
            accounts
        );
        assert!(registry
            .reserve_missing_accounts("profile-a", &accounts)
            .unwrap()
            .is_empty());
        assert_eq!(
            registry.account_states("profile-a", &accounts),
            AccountStateCounts {
                selected: 2,
                pending: 2,
                ..AccountStateCounts::default()
            }
        );
    }

    #[test]
    fn successful_spawn_promotes_reservation_and_failed_spawn_can_release_it() {
        let mut registry = SessionRegistry::default();
        let accounts = vec!["account-a".to_string(), "account-b".to_string()];
        registry
            .reserve_missing_accounts("profile-a", &accounts)
            .unwrap();

        registry
            .register("profile-a", "account-a", spawn_sleeper(), unix_time_ms())
            .unwrap();
        registry.release_reservation("profile-a", "account-b");

        assert_eq!(
            registry.account_states("profile-a", &accounts),
            AccountStateCounts {
                selected: 2,
                active: 1,
                missing: 1,
                ..AccountStateCounts::default()
            }
        );
        registry.kill_all_for_test();
    }

    #[test]
    fn returns_identity_only_while_runtime_session_is_active() {
        let mut registry = SessionRegistry::default();
        let child = spawn_sleeper();
        let pid = child.id();
        let runtime_session_id = registry
            .register("profile-a", "account-a", child, unix_time_ms())
            .unwrap();

        let identity = registry
            .active_process_identity(runtime_session_id)
            .unwrap();
        assert_eq!(identity.runtime_session_id, runtime_session_id);
        assert_eq!(identity.pid, pid);
        #[cfg(windows)]
        assert!(identity.process_creation_time_filetime_100ns.is_some());

        registry.kill_all_for_test();
        assert!(registry
            .active_process_identity(runtime_session_id)
            .is_err());
    }

    #[test]
    fn exited_child_moves_to_recent_exit_on_snapshot() {
        let mut registry = SessionRegistry::default();
        let runtime_session_id = registry
            .register(
                "profile-a",
                "account-a",
                spawn_with_exit_code(7),
                unix_time_ms(),
            )
            .unwrap();

        let mut snapshot = registry.snapshot();
        for _ in 0..100 {
            if snapshot.active_sessions.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
            snapshot = registry.snapshot();
        }

        assert!(snapshot.active_sessions.is_empty());
        assert_eq!(snapshot.recent_exits.len(), 1);
        assert_eq!(
            snapshot.recent_exits[0].runtime_session_id,
            runtime_session_id
        );
        assert_eq!(snapshot.recent_exits[0].exit_code, Some(7));
        assert!(registry.by_account.is_empty());
    }

    #[test]
    fn records_elevated_fallback_without_credentials_or_arguments() {
        let mut registry = SessionRegistry::default();
        registry
            .reserve_missing_accounts("profile-a", &["account-a".to_string()])
            .unwrap();
        registry.record_untracked_elevated_fallback("profile-a", Some("account-a"), unix_time_ms());

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.untracked_launches.len(), 1);
        assert_eq!(
            snapshot.untracked_launches[0].code,
            "untracked_elevated_fallback"
        );
        assert_eq!(
            snapshot.untracked_launches[0].account_id.as_deref(),
            Some("account-a")
        );
        assert_eq!(
            registry.account_states("profile-a", &["account-a".to_string()]),
            AccountStateCounts {
                selected: 1,
                untracked: 1,
                ..AccountStateCounts::default()
            }
        );
    }
}
