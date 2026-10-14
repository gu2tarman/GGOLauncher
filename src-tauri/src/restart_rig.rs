//! Opt-in real-process rig. Uses the same launch/status APIs as the UI.
use crate::{launcher, multiclient::Stage0State};
use serde_json::json;
use std::{fs, io::Write, path::PathBuf, thread, time::Duration};

#[test]
#[ignore = "requires isolated ServUO and GGO_RESTART_RIG_SETTINGS"]
fn unattended_restart_rig() {
    let root = PathBuf::from(std::env::var("GGO_RESTART_RIG_OUTPUT").unwrap());
    let state = Stage0State::from_environment();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let launch = runtime
        .block_on(launcher::launch_multi("restart-rig", 300, &state))
        .unwrap();
    fs::write(
        root.join("launch.json"),
        serde_json::to_vec_pretty(&launch).unwrap(),
    )
    .unwrap();
    if let Ok(account) = std::env::var("GGO_RESTART_RIG_REJECT") {
        state.rig_reject_restart(&account);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(240);
    let mut sequence = 0;
    let mut timeline = fs::File::create(root.join("timeline.jsonl")).unwrap();
    while std::time::Instant::now() < deadline && !root.join("stop").exists() {
        let control = root.join("multi.request");
        if control.exists() {
            fs::remove_file(control).unwrap();
            let result = runtime.block_on(launcher::launch_multi("restart-rig", 300, &state));
            fs::write(
                root.join("multi.result.json"),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
        }
        let status = launcher::multi_session_status("restart-rig", &state).unwrap();
        let evidence = json!({"sequence":sequence, "status":status, "diagnostics":state.diagnostics(),
                              "party":state.rig_party_status()});
        writeln!(timeline, "{}", json!({"sequence":sequence,"status":status,
            "sessions":evidence["diagnostics"]["registry"]["active_sessions"],
            "warnings":evidence["diagnostics"]["registry"]["warnings"]})).unwrap();
        timeline.flush().unwrap();
        let pending = root.join("launcher.tmp");
        fs::write(&pending, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        // MoveFileEx replaces atomically on Windows.
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = pending.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = root
            .join("launcher.json")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(from.as_ptr(), target.as_ptr(), 1);
        }
        sequence += 1;
        thread::sleep(Duration::from_millis(200));
    }
    state.rig_kill_clients();
}
