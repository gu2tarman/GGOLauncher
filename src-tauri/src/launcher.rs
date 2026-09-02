use crate::multiclient::{
    launcher_observed_time_ms, BrokerBootstrap, ManagedTile, MultiSessionStatus, Stage0State,
};
use crate::profile::{Account, Profile, SecondaryLayoutPreset, SecondarySlot};
use crate::settings;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Child;

enum SpawnOutcome {
    NormalChild {
        child: Child,
        launcher_observed_spawn_time_unix_ms: u64,
    },
    UntrackedElevatedFallback {
        launcher_observed_spawn_time_unix_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MultiLaunchResult {
    pub selected_count: usize,
    pub launched_count: usize,
    pub already_running_count: usize,
    pub layout_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupControlAccountResult {
    pub account_id: String,
    pub slot: &'static str,
    pub status: &'static str,
    pub message: Option<String>,
    pub window: Option<crate::multiclient::WindowControlObservation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupControlResult {
    pub action: &'static str,
    pub succeeded_count: usize,
    pub pending_count: usize,
    pub failed_count: usize,
    pub accounts: Vec<GroupControlAccountResult>,
}

#[derive(Debug)]
struct ManagedTilePlan {
    tiles: Vec<Option<ManagedTile>>,
    warning: Option<String>,
}

/// settings.ui.first_launch_completed 마킹 (이미 true면 no-op).
/// 실패해도 silent — 부가 기능이라 실행 흐름은 막지 않음.
fn mark_first_launch_completed() {
    if let Ok(mut s) = settings::load() {
        if !s.ui.first_launch_completed {
            s.ui.first_launch_completed = true;
            let _ = settings::save(&s);
        }
    }
}

/// 단일 PLAY — 활성 계정(있으면)으로 CUO 1회 실행.
/// account_override가 Some이면 그 계정 사용, None이면 무인증(사용자가 CUO에서 직접 입력).
pub fn launch(
    profile_id: &str,
    account_override: Option<&Account>,
    stage0: &Stage0State,
) -> Result<(), String> {
    let s = settings::load()?;
    let (profile, plugin, cuo_dir, cuo_exe, plugin_path) = resolve_launch_context(&s, profile_id)?;
    let _ = plugin_path;

    let args = build_cuo_args(&profile, &plugin.path, account_override, None);
    let outcome = spawn_hidden(&cuo_exe, &cuo_dir, &args, None)?;
    observe_stage0_spawn(
        stage0,
        profile_id,
        account_override.map(|account| account.id.as_str()),
        outcome,
        None,
    );
    mark_first_launch_completed();
    Ok(())
}

/// 계정의 multi_enabled 정책 해석. None(레거시)이면 인덱스 기반 기본값 적용:
/// 첫 6개 = true, 7번째부터 = false.
fn is_account_multi_enabled(account: &Account, index: usize) -> bool {
    match account.multi_enabled {
        Some(v) => v,
        None => index < 6,
    }
}

fn selected_multi_accounts(profile: &Profile) -> Vec<&Account> {
    profile
        .server
        .accounts
        .iter()
        .enumerate()
        .filter(|(index, account)| is_account_multi_enabled(account, *index))
        .map(|(_, account)| account)
        .take(6)
        .collect()
}

pub fn multi_session_status(
    profile_id: &str,
    stage0: &Stage0State,
) -> Result<MultiSessionStatus, String> {
    let settings = settings::load()?;
    let profile = settings
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "프로필을 찾을 수 없습니다.".to_string())?;
    let account_ids = selected_multi_accounts(profile)
        .into_iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    stage0.multi_session_status(profile_id, &account_ids)
}

pub fn control_secondary_group(
    profile_id: &str,
    action: &str,
    stage0: &Stage0State,
) -> Result<GroupControlResult, String> {
    let settings = settings::load()?;
    let profile = settings
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "프로필을 찾을 수 없습니다.".to_string())?;
    let selected = selected_multi_accounts(profile);
    let leader_account_id = selected.first().map(|account| account.id.clone());
    let mut accounts = selected
        .into_iter()
        .filter(|account| {
            account.secondary_slot.is_some() && Some(&account.id) != leader_account_id.as_ref()
        })
        .collect::<Vec<_>>();
    accounts.sort_by_key(|account| account.secondary_slot.unwrap().order());
    if accounts.is_empty() {
        return Err("현재 MULTI 대상에 보조 모니터 슬롯이 지정된 계정이 없습니다.".to_string());
    }
    if !matches!(
        action,
        "minimize" | "restore_preset" | "group_raise" | "close_secondary"
    ) {
        return Err(format!("지원하지 않는 보조창 그룹 동작입니다: {action}"));
    }

    let mut results = Vec::with_capacity(accounts.len());
    for account in accounts {
        let slot = account.secondary_slot.unwrap().as_str();
        let target = match stage0.broker_window_target(profile_id, &account.id) {
            Ok(target) => target,
            Err(error) => {
                results.push(GroupControlAccountResult {
                    account_id: account.id.clone(),
                    slot,
                    status: "pending",
                    message: Some(error),
                    window: None,
                });
                continue;
            }
        };

        let operation = match action {
            "minimize" => crate::multiclient::minimize_broker_window(target),
            "restore_preset" => {
                crate::multiclient::restore_broker_window(target).and_then(|window| {
                    stage0.request_apply_managed_tile(profile_id, &account.id)?;
                    Ok(window)
                })
            }
            "group_raise" => crate::multiclient::raise_broker_window(target),
            "close_secondary" => crate::multiclient::close_broker_window(target),
            _ => unreachable!(),
        };
        match operation {
            Ok(window) => results.push(GroupControlAccountResult {
                account_id: account.id.clone(),
                slot,
                status: "success",
                message: None,
                window: Some(window),
            }),
            Err(error) => results.push(GroupControlAccountResult {
                account_id: account.id.clone(),
                slot,
                status: "failed",
                message: Some(error),
                window: None,
            }),
        }
    }

    Ok(GroupControlResult {
        action: match action {
            "minimize" => "minimize",
            "restore_preset" => "restore_preset",
            "group_raise" => "group_raise",
            "close_secondary" => "close_secondary",
            _ => unreachable!(),
        },
        succeeded_count: results
            .iter()
            .filter(|result| result.status == "success")
            .count(),
        pending_count: results
            .iter()
            .filter(|result| result.status == "pending")
            .count(),
        failed_count: results
            .iter()
            .filter(|result| result.status == "failed")
            .count(),
        accounts: results,
    })
}

/// MULTI LOGIN – 프로필 안의 multi_enabled 계정만 순차 spawn.
/// 각 spawn 사이 `delay_ms` 만큼 대기 (서버의 동일 IP 연속 접속 거부 방지).
/// async: tokio::sleep 사용해서 Tauri IPC/webview 스레드 안 막음 (안 그러면 응답 없음).
pub async fn launch_multi(
    profile_id: &str,
    delay_ms: u64,
    stage0: &Stage0State,
) -> Result<MultiLaunchResult, String> {
    let s = settings::load()?;
    let (profile, plugin, cuo_dir, cuo_exe, _) = resolve_launch_context(&s, profile_id)?;

    if profile.server.accounts.is_empty() {
        return Err("등록된 계정이 없습니다. Edit Profile에서 계정을 추가해주세요.".to_string());
    }

    // multi_enabled 필터 + 최대 6개 cap (UI 우회 방어).
    let selected = selected_multi_accounts(&profile);

    if selected.is_empty() {
        return Err(
            "MULTI LOGIN 대상 계정이 없습니다. Edit Profile에서 계정을 선택해주세요.".to_string(),
        );
    }

    // 모든 검증과 모니터 계산을 첫 spawn 전에 끝낸다. 중복 슬롯이나
    // 세컨 모니터 부재로 일부 계정만 실행되는 상태를 만들지 않는다.
    let managed_tile_plan = managed_tiles_for_accounts(&selected, profile.secondary_layout_preset)?;
    let account_ids = selected
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let reserved_ids = stage0.reserve_missing_accounts(profile_id, &account_ids)?;
    let reserved_set = reserved_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut launch_plan = selected
        .iter()
        .zip(managed_tile_plan.tiles)
        .filter_map(|(account, tile)| {
            reserved_set
                .contains(account.id.as_str())
                .then_some((*account, tile))
        })
        .collect::<Vec<_>>();
    // The centered tile overlaps all four base cells by design. Keep the
    // existing account/login order, but launch that one window last so it is
    // visible on top immediately after a full MULTI start.
    launch_plan.sort_by_key(|(account, _)| account.secondary_slot == Some(SecondarySlot::Center));
    let already_running_count = selected.len().saturating_sub(launch_plan.len());
    // Keep HUD identity independent from any current or future window preset.
    // No new leader setting or automatic failover in the first completion:
    // the first MULTI account is the stable leader, and every other connected
    // MULTI account is eligible for the HUD regardless of window placement.
    let hud_leader_account_id = hud_leader_account_id(&selected)
        .ok_or_else(|| "HUD 리더 계정을 결정할 수 없습니다.".to_string())?;

    let mut launched_count = 0usize;
    for (i, (account, managed_tile)) in launch_plan.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        let hud_order = selected
            .iter()
            .position(|selected_account| selected_account.id == account.id)
            .and_then(|index| u8::try_from(index).ok())
            .ok_or_else(|| format!("{}: HUD 표시 순서를 결정할 수 없습니다.", account.id))?;
        let bootstrap = match stage0.prepare_broker_session(
            profile_id,
            &account.id,
            account.id == hud_leader_account_id,
            hud_order,
            account.secondary_slot.map(|slot| slot.order()),
        ) {
            Ok(bootstrap) => bootstrap,
            Err(error) => {
                for (remaining, _) in launch_plan.iter().skip(i) {
                    stage0.release_account_reservation(profile_id, &remaining.id);
                }
                return Err(format!("CE IPC 세션 준비 실패: {error}"));
            }
        };
        let args = build_cuo_args(&profile, &plugin.path, Some(account), *managed_tile);
        let outcome = match spawn_hidden(&cuo_exe, &cuo_dir, &args, Some(&bootstrap)) {
            Ok(outcome) => outcome,
            Err(error) => {
                stage0.cancel_broker_session(&bootstrap.launch_session_id);
                for (remaining, _) in launch_plan.iter().skip(i) {
                    stage0.release_account_reservation(profile_id, &remaining.id);
                }
                return Err(error);
            }
        };
        observe_stage0_spawn(
            stage0,
            profile_id,
            Some(account.id.as_str()),
            outcome,
            Some(&bootstrap),
        );
        launched_count += 1;
    }
    if launched_count > 0 {
        mark_first_launch_completed();
    }
    Ok(MultiLaunchResult {
        selected_count: selected.len(),
        launched_count,
        already_running_count,
        layout_warning: managed_tile_plan.warning,
    })
}

fn hud_leader_account_id<'a>(selected: &[&'a Account]) -> Option<&'a str> {
    selected.first().map(|account| account.id.as_str())
}

fn managed_tiles_for_accounts(
    selected: &[&Account],
    preset: SecondaryLayoutPreset,
) -> Result<ManagedTilePlan, String> {
    let mut assigned = HashSet::new();
    for account in selected {
        if let Some(slot) = account.secondary_slot {
            if !preset.allows(slot) {
                return Err(format!(
                    "현재 보조 모니터 프리셋에서는 {} 슬롯을 사용할 수 없습니다.",
                    slot.as_str()
                ));
            }
            if !assigned.insert(slot) {
                return Err(format!(
                    "세컨 모니터 배치 슬롯 {}이(가) 중복 지정되었습니다. 프로필 편집에서 각 계정에 서로 다른 슬롯을 선택해주세요.",
                    slot.as_str()
                ));
            }
        }
    }

    if assigned.is_empty() {
        return Ok(ManagedTilePlan {
            tiles: vec![None; selected.len()],
            warning: None,
        });
    }

    let available_tiles = crate::multiclient::secondary_managed_tiles(preset)?;
    Ok(managed_tile_plan_from_available(
        selected,
        available_tiles.as_ref(),
    ))
}

fn managed_tile_plan_from_available(
    selected: &[&Account],
    available_tiles: Option<&std::collections::HashMap<&'static str, ManagedTile>>,
) -> ManagedTilePlan {
    let Some(available_tiles) = available_tiles else {
        return ManagedTilePlan {
            tiles: vec![None; selected.len()],
            warning: Some(
                "보조 모니터를 찾지 못해 이번 실행에서는 창 자동 배치를 적용하지 않았습니다. 저장된 슬롯 설정은 유지됩니다."
                    .to_string(),
            ),
        };
    };

    let tiles = selected
        .iter()
        .map(|account| {
            account
                .secondary_slot
                .and_then(|slot| available_tiles.get(slot.as_str()).copied())
        })
        .collect();
    ManagedTilePlan {
        tiles,
        warning: None,
    }
}

fn observe_stage0_spawn(
    stage0: &Stage0State,
    profile_id: &str,
    account_id: Option<&str>,
    outcome: SpawnOutcome,
    bootstrap: Option<&BrokerBootstrap>,
) {
    match outcome {
        SpawnOutcome::NormalChild {
            child,
            launcher_observed_spawn_time_unix_ms,
        } => {
            if let Some(bootstrap) = bootstrap {
                if let Err(error) = stage0.bind_broker_process(bootstrap, child.id()) {
                    stage0.cancel_broker_session(&bootstrap.launch_session_id);
                    eprintln!("[multiclient] broker process binding failed: {error}");
                }
            }
            if let Some(account_id) = account_id {
                stage0.observe_spawn(
                    profile_id,
                    account_id,
                    child,
                    launcher_observed_spawn_time_unix_ms,
                );
            }
        }
        SpawnOutcome::UntrackedElevatedFallback {
            launcher_observed_spawn_time_unix_ms,
        } => {
            if let Some(bootstrap) = bootstrap {
                stage0.cancel_broker_session(&bootstrap.launch_session_id);
            }
            stage0.observe_untracked_elevated_fallback(
                profile_id,
                account_id,
                launcher_observed_spawn_time_unix_ms,
            )
        }
    }
}

/// 공용 사전조건 검증 + 경로/플러그인 resolve.
fn resolve_launch_context(
    s: &crate::settings::Settings,
    profile_id: &str,
) -> Result<
    (
        Profile,
        crate::settings::PluginEntry,
        PathBuf,
        PathBuf,
        PathBuf,
    ),
    String,
> {
    let profile = s
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("프로필 {profile_id} 못 찾음"))?
        .clone();

    let plugin = s
        .plugins
        .iter()
        .find(|p| p.enabled)
        .ok_or_else(|| {
            "선택된 플러그인이 없습니다. 플러그인을 먼저 등록/선택해주세요.".to_string()
        })?
        .clone();

    let cuo_dir_str = profile
        .cuo_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "CUO 경로가 비어있습니다. Edit Profile에서 설정해주세요.".to_string())?;
    let cuo_dir = PathBuf::from(cuo_dir_str);
    if !cuo_dir.is_dir() {
        return Err(format!("CUO 경로가 폴더가 아닙니다: {}", cuo_dir.display()));
    }
    let cuo_exe = find_cuo_exe(&cuo_dir)
        .ok_or_else(|| format!("ClassicUO.exe를 찾지 못함: {}", cuo_dir.display()))?;

    let plugin_path = PathBuf::from(&plugin.path);
    if !plugin_path.exists() {
        return Err(format!(
            "플러그인 파일이 없습니다: {}",
            plugin_path.display()
        ));
    }

    Ok((profile, plugin, cuo_dir, cuo_exe, plugin_path))
}

fn find_cuo_exe(dir: &Path) -> Option<PathBuf> {
    for name in ["ClassicUO.exe", "classicuo.exe", "CLASSICUO.EXE"] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// ClassicUO 명령행 인자 조립. 원본 ClassicUOLauncher가 spawn 시 사용하는 인자 형식 그대로.
/// 캡처: re_spawn_capture.log 참조.
/// 인자 prefix는 single-dash (-ip, -port 등). 공백 포함 값은 따옴표 wrap.
/// account가 Some이면 -account/-password + -autologin True 추가, None이면 무인증.
fn build_cuo_args(
    profile: &crate::profile::Profile,
    plugin_path: &str,
    account: Option<&Account>,
    managed_tile: Option<ManagedTile>,
) -> String {
    let mut out: Vec<String> = Vec::new();

    // UO 클라이언트 폴더
    let uo = profile.uo_path.trim();
    if !uo.is_empty() {
        out.push("-uopath".into());
        out.push(q(uo));
    }

    // 서버
    let addr = profile.server.address.trim();
    if !addr.is_empty() {
        out.push("-ip".into());
        out.push(q(addr));
        out.push("-port".into());
        out.push(profile.server.port.to_string());
    }

    // 클라이언트 버전 (값 있을 때만 — 비우면 CUO 자동감지)
    if let Some(v) = profile.client_version.as_deref().map(str::trim) {
        if !v.is_empty() {
            out.push("-clientversion".into());
            out.push(q(v));
        }
    }

    // 계정 정보 — 로그인 화면 자동 채움용.
    // CUO 플래그 구조상 -autologin은 (로그인+서버+캐릭) 자동선택 ALL-OR-NOTHING.
    // 캐릭은 사용자가 골라야 하므로 -autologin False 유지. 로그인 화면에 creds만 채워짐.
    // 비밀번호는 settings.json에 DPAPI로 암호화돼 있어서 spawn 전 복호화 필요.
    let mut has_character_target = false;
    if let Some(a) = account {
        if !a.username.trim().is_empty() {
            out.push("-username".into());
            out.push(q(a.username.trim()));
            if !a.password_encrypted.trim().is_empty() {
                let pw_plain = crate::crypto::decrypt_or_passthrough(&a.password_encrypted);
                if !pw_plain.is_empty() {
                    out.push("-password".into());
                    out.push(q(&pw_plain));
                }
            }
            if let Some(character_name) = a.character_name.as_deref().map(str::trim) {
                if !character_name.is_empty() {
                    out.push("-lastcharactername".into());
                    out.push(q(character_name));
                    has_character_target = true;
                }
            }
        }
    }

    push_kv(&mut out, "-saveaccount", "False");
    push_kv(
        &mut out,
        "-autologin",
        if has_character_target {
            "True"
        } else {
            "False"
        },
    );
    push_kv(&mut out, "-reconnect", "False");
    push_kv(&mut out, "-reconnect_time", "1000");
    push_kv(&mut out, "-music", "True");
    push_kv(&mut out, "-music_volume", "50");
    out.push("-skiploginscreen".into());
    push_kv(&mut out, "-profiler", "False");
    push_kv(&mut out, "-use_verdata", "False");

    // 암호화 매핑 (EncryptionType → CUO 정수). 원본 기본 0=Auto.
    let enc_num = match profile.server.encryption {
        crate::profile::EncryptionType::Auto => "0",
        crate::profile::EncryptionType::None => "1",
        crate::profile::EncryptionType::OldBlowfish => "2",
        crate::profile::EncryptionType::Blowfish125 => "3",
        crate::profile::EncryptionType::Twofish => "4",
    };
    push_kv(&mut out, "-encryption", enc_num);

    push_kv(&mut out, "-force_driver", "3");

    if let Some(tile) = managed_tile {
        out.push("-ggo_managed_tile".into());
        out.push(managed_tile_value(tile));
    }

    // 플러그인 (.dll이든 .exe이든 똑같이 -plugins 인자로 넘김)
    out.push("-plugins".into());
    out.push(q(plugin_path));

    push_kv(&mut out, "-shard_type", "0");
    out.push("-skipupdate".into());

    out.join(" ")
}

fn managed_tile_value(tile: ManagedTile) -> String {
    format!(
        "x={};y={};w={};h={}",
        tile.left, tile.top, tile.width, tile.height
    )
}

fn push_kv(out: &mut Vec<String>, key: &str, val: &str) {
    out.push(key.into());
    out.push(val.into());
}

/// Windows CommandLineToArgvW 규칙에 맞춘 quoting.
/// - 공백/탭/따옴표 없으면 그대로
/// - 있으면 `"..."` 감싸기. 내부 `"`는 `\"`. 따옴표 직전의 연속 `\`는 두 배.
///   (특히 경로가 `C:\foo\`처럼 백슬래시로 끝나는 케이스에서
///    naive `"C:\foo\"`는 닫는 따옴표를 escape해버려 인자 병합 발생.)
fn q(s: &str) -> String {
    if !s.chars().any(|c| c.is_whitespace() || c == '"') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // 연속 백슬래시 카운트
            let mut bs = 0;
            while i < chars.len() && chars[i] == '\\' {
                bs += 1;
                i += 1;
            }
            // 다음 글자가 " 또는 끝(닫는 따옴표 직전)이면 백슬래시 두 배
            if i == chars.len() {
                for _ in 0..(bs * 2) {
                    out.push('\\');
                }
            } else if chars[i] == '"' {
                for _ in 0..(bs * 2) {
                    out.push('\\');
                }
                out.push('\\');
                out.push('"');
                i += 1;
            } else {
                for _ in 0..bs {
                    out.push('\\');
                }
            }
        } else if c == '"' {
            out.push('\\');
            out.push('"');
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.push('"');
    out
}

// ── 콘솔 숨김 spawn (Command + CREATE_NO_WINDOW) ─────────────
/// CONSOLE 서브시스템 앱(CUO 등)을 콘솔 창 없이 실행.
/// elevation 필요한 exe는 ShellExecute로 폴백.
#[cfg(windows)]
fn spawn_hidden(
    exe: &Path,
    working_dir: &Path,
    args: &str,
    bootstrap: Option<&BrokerBootstrap>,
) -> Result<SpawnOutcome, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 우리는 args를 한 문자열로 조립했음. CUO 같은 케이스에선
    // raw_arg로 통째 넘기는 게 quote escape 일관성 유지.
    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(working_dir);
    if !args.is_empty() {
        cmd.raw_arg(args);
    }
    cmd.creation_flags(CREATE_NO_WINDOW);
    apply_broker_environment(&mut cmd, bootstrap);

    match cmd.spawn() {
        Ok(child) => Ok(SpawnOutcome::NormalChild {
            child,
            launcher_observed_spawn_time_unix_ms: launcher_observed_time_ms(),
        }),
        Err(e) => {
            // ERROR_ELEVATION_REQUIRED (740) 이면 ShellExecute로 폴백
            if e.raw_os_error() == Some(740) {
                shell_execute(exe, working_dir, args)?;
                return Ok(SpawnOutcome::UntrackedElevatedFallback {
                    launcher_observed_spawn_time_unix_ms: launcher_observed_time_ms(),
                });
            }
            Err(format!("실행 실패: {e}"))
        }
    }
}

#[cfg(not(windows))]
fn spawn_hidden(
    exe: &Path,
    working_dir: &Path,
    args: &str,
    bootstrap: Option<&BrokerBootstrap>,
) -> Result<SpawnOutcome, String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(working_dir);
    if !args.is_empty() {
        cmd.arg(args);
    }
    apply_broker_environment(&mut cmd, bootstrap);
    cmd.spawn()
        .map(|child| SpawnOutcome::NormalChild {
            child,
            launcher_observed_spawn_time_unix_ms: launcher_observed_time_ms(),
        })
        .map_err(|error| error.to_string())
}

fn apply_broker_environment(
    command: &mut std::process::Command,
    bootstrap: Option<&BrokerBootstrap>,
) {
    let Some(bootstrap) = bootstrap else {
        return;
    };
    command
        .env("GGO_BROKER_PIPE", &bootstrap.pipe_name)
        .env("GGO_LAUNCH_SESSION_ID", &bootstrap.launch_session_id)
        .env("GGO_BOOTSTRAP_TOKEN", &bootstrap.bootstrap_token)
        .env("GGO_PROFILE_ID", &bootstrap.profile_id)
        .env("GGO_ACCOUNT_ID", &bootstrap.account_id);
}

// ── Windows ShellExecuteExW ─────────────────────────────────
#[cfg(windows)]
fn shell_execute(exe: &Path, working_dir: &Path, args: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(once(0)).collect()
    }

    let exe_w = to_wide(&exe.to_string_lossy());
    let dir_w = to_wide(&working_dir.to_string_lossy());
    let args_w = to_wide(args);
    let verb_w = to_wide("open");

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC;
    info.lpVerb = verb_w.as_ptr();
    info.lpFile = exe_w.as_ptr();
    info.lpParameters = if args.is_empty() {
        std::ptr::null()
    } else {
        args_w.as_ptr()
    };
    info.lpDirectory = dir_w.as_ptr();
    info.nShow = SW_SHOWNORMAL as i32;

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(format!(
            "실행 실패 (Windows error {err}): {}",
            explain_win_error(err)
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn explain_win_error(code: u32) -> &'static str {
    match code {
        2 => "파일을 찾을 수 없음 (ERROR_FILE_NOT_FOUND)",
        3 => "경로를 찾을 수 없음 (ERROR_PATH_NOT_FOUND)",
        5 => "접근 거부됨 (ERROR_ACCESS_DENIED)",
        740 => "권한 상승 필요 — UAC 프롬프트 거부됨 (ERROR_ELEVATION_REQUIRED)",
        1223 => "사용자가 UAC 프롬프트 취소 (ERROR_CANCELLED)",
        _ => "원인 미상",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::SecondarySlot;

    fn account(id: &str, slot: Option<SecondarySlot>) -> Account {
        Account {
            id: id.to_string(),
            username: id.to_string(),
            password_encrypted: String::new(),
            character_name: None,
            display_name: None,
            multi_enabled: Some(true),
            secondary_slot: slot,
        }
    }

    #[test]
    fn duplicate_managed_slots_fail_before_monitor_resolution() {
        let first = account("first", Some(SecondarySlot::R0c0));
        let second = account("second", Some(SecondarySlot::R0c0));

        let error = managed_tiles_for_accounts(&[&first, &second], SecondaryLayoutPreset::TwoByTwo)
            .unwrap_err();

        assert!(error.contains("r0c0"));
        assert!(error.contains("중복"));
    }

    #[test]
    fn accounts_without_slots_do_not_require_a_secondary_monitor() {
        let first = account("first", None);
        let second = account("second", None);

        let tiles = managed_tiles_for_accounts(&[&first, &second], SecondaryLayoutPreset::TwoByTwo)
            .unwrap();

        assert_eq!(tiles.tiles, vec![None, None]);
        assert!(tiles.warning.is_none());
    }

    #[test]
    fn hud_leader_and_member_order_do_not_depend_on_window_slots() {
        let first = account("main", Some(SecondarySlot::R1c1));
        let second = account("bard", None);

        assert_eq!(hud_leader_account_id(&[&first, &second]), Some("main"));
        assert_eq!(hud_leader_account_id(&[&second, &first]), Some("bard"));
    }

    #[test]
    fn missing_secondary_monitor_falls_back_to_unmanaged_without_losing_slots() {
        let first = account("first", Some(SecondarySlot::R0c0));
        let second = account("second", None);

        let plan = managed_tile_plan_from_available(&[&first, &second], None);

        assert_eq!(plan.tiles, vec![None, None]);
        assert!(plan
            .warning
            .as_deref()
            .unwrap()
            .contains("자동 배치를 적용하지 않았습니다"));
    }

    #[test]
    fn center_slot_requires_the_center_preset() {
        let center = account("center", Some(SecondarySlot::Center));

        let error =
            managed_tiles_for_accounts(&[&center], SecondaryLayoutPreset::TwoByTwo).unwrap_err();

        assert!(error.contains("center"));
        assert!(error.contains("사용할 수 없습니다"));
    }

    #[test]
    fn managed_tile_argument_preserves_negative_coordinates() {
        let value = managed_tile_value(ManagedTile {
            left: -1920,
            top: 243,
            width: 960,
            height: 516,
        });

        assert_eq!(value, "x=-1920;y=243;w=960;h=516");
        assert!(!value.starts_with('-'));
    }
}
