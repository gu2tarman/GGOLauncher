use crate::profile::{Account, Profile};
use crate::settings;
use std::path::{Path, PathBuf};

/// 단일 PLAY — 활성 계정(있으면)으로 CUO 1회 실행.
/// account_override가 Some이면 그 계정 사용, None이면 무인증(사용자가 CUO에서 직접 입력).
pub fn launch(profile_id: &str, account_override: Option<&Account>) -> Result<(), String> {
    let s = settings::load()?;
    let (profile, plugin, cuo_dir, cuo_exe, plugin_path) = resolve_launch_context(&s, profile_id)?;
    let _ = plugin_path;

    let args = build_cuo_args(&profile, &plugin.path, account_override);
    spawn_hidden(&cuo_exe, &cuo_dir, &args)?;
    Ok(())
}

/// MULTI LOGIN — 프로필 안의 계정 N개를 순차 spawn.
/// 각 spawn 사이 `delay_ms` 만큼 대기 (서버의 동일 IP 연속 접속 거부 방지).
/// async: tokio::sleep 사용해서 Tauri IPC/webview 스레드 안 막음 (안 그러면 응답 없음).
pub async fn launch_multi(profile_id: &str, delay_ms: u64) -> Result<usize, String> {
    let s = settings::load()?;
    let (profile, plugin, cuo_dir, cuo_exe, _) = resolve_launch_context(&s, profile_id)?;

    if profile.server.accounts.is_empty() {
        return Err("등록된 계정이 없습니다. Edit Profile에서 계정을 추가해주세요.".to_string());
    }

    let mut count = 0usize;
    for (i, account) in profile.server.accounts.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        let args = build_cuo_args(&profile, &plugin.path, Some(account));
        spawn_hidden(&cuo_exe, &cuo_dir, &args)?;
        count += 1;
    }
    Ok(count)
}


/// 공용 사전조건 검증 + 경로/플러그인 resolve.
fn resolve_launch_context(
    s: &crate::settings::Settings,
    profile_id: &str,
) -> Result<(Profile, crate::settings::PluginEntry, PathBuf, PathBuf, PathBuf), String> {
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
        .ok_or_else(|| "선택된 플러그인이 없습니다. 플러그인을 먼저 등록/선택해주세요.".to_string())?
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
        return Err(format!("플러그인 파일이 없습니다: {}", plugin_path.display()));
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
        }
    }

    push_kv(&mut out, "-saveaccount", "False");
    push_kv(&mut out, "-autologin", "False");
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

    // 플러그인 (.dll이든 .exe이든 똑같이 -plugins 인자로 넘김)
    out.push("-plugins".into());
    out.push(q(plugin_path));

    push_kv(&mut out, "-shard_type", "0");
    out.push("-skipupdate".into());

    out.join(" ")
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
                for _ in 0..(bs * 2) { out.push('\\'); }
            } else if chars[i] == '"' {
                for _ in 0..(bs * 2) { out.push('\\'); }
                out.push('\\');
                out.push('"');
                i += 1;
            } else {
                for _ in 0..bs { out.push('\\'); }
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
fn spawn_hidden(exe: &Path, working_dir: &Path, args: &str) -> Result<(), String> {
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

    match cmd.spawn() {
        Ok(_) => Ok(()),
        Err(e) => {
            // ERROR_ELEVATION_REQUIRED (740) 이면 ShellExecute로 폴백
            if e.raw_os_error() == Some(740) {
                return shell_execute(exe, working_dir, args);
            }
            Err(format!("실행 실패: {e}"))
        }
    }
}

#[cfg(not(windows))]
fn spawn_hidden(exe: &Path, working_dir: &Path, args: &str) -> Result<(), String> {
    shell_execute(exe, working_dir, args)
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
    info.lpParameters = if args.is_empty() { std::ptr::null() } else { args_w.as_ptr() };
    info.lpDirectory = dir_w.as_ptr();
    info.nShow = SW_SHOWNORMAL as i32;

    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(format!("실행 실패 (Windows error {err}): {}", explain_win_error(err)));
    }
    Ok(())
}

#[cfg(not(windows))]
fn shell_execute(exe: &Path, working_dir: &Path, args: &str) -> Result<(), String> {
    // 비-Windows에서는 표준 spawn으로 폴백 (개발 편의용)
    let mut cmd = std::process::Command::new(exe);
    cmd.current_dir(working_dir);
    if !args.is_empty() {
        cmd.arg(args);
    }
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
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
