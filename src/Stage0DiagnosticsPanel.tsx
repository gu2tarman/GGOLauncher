import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type {
  Stage0Diagnostics,
  Stage0SignedRect,
} from "./types";

const formatRect = (rect: Stage0SignedRect) =>
  `(${rect.left}, ${rect.top})–(${rect.right}, ${rect.bottom})`;

const formatTime = (unixMs: number) =>
  new Date(unixMs).toLocaleTimeString("ko-KR", { hour12: false });

const windowStatusLabel = (status: string) => {
  switch (status) {
    case "ready": return "게임창 준비";
    case "pending_game_window": return "게임창 대기";
    case "ambiguous_game_windows": return "게임창 복수 후보";
    case "process_identity_mismatch": return "프로세스 불일치";
    case "process_identity_unverified": return "프로세스 미검증";
    case "unsupported_platform": return "지원하지 않는 OS";
    default: return status;
  }
};

export function Stage0DiagnosticsPanel() {
  const [diagnostics, setDiagnostics] = useState<Stage0Diagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [windowActionBusy, setWindowActionBusy] = useState<number | null>(null);
  const [windowActionMessage, setWindowActionMessage] = useState<{
    kind: "ok" | "error";
    text: string;
  } | null>(null);
  const [brokerActionBusy, setBrokerActionBusy] = useState<string | null>(null);
  const [brokerActionMessage, setBrokerActionMessage] = useState<{
    kind: "ok" | "error";
    text: string;
  } | null>(null);

  const refresh = useCallback(async (showSpinner = false) => {
    if (showSpinner) setRefreshing(true);
    try {
      const next = await api.multiclientStage0Diagnostics();
      setDiagnostics(next);
      setError(null);
      return next;
    } catch (requestError) {
      setError(String(requestError));
      return null;
    } finally {
      if (showSpinner) setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let intervalId: number | null = null;

    const start = async () => {
      const initial = await refresh();
      if (!disposed && initial?.enabled) {
        intervalId = window.setInterval(() => void refresh(), 1000);
      }
    };
    void start();

    return () => {
      disposed = true;
      if (intervalId !== null) window.clearInterval(intervalId);
    };
  }, [refresh]);

  const moveSingleWindowTest = async (runtimeSessionId: number) => {
    setWindowActionBusy(runtimeSessionId);
    setWindowActionMessage(null);
    try {
      const result = await api.multiclientStage1BMoveTest(runtimeSessionId);
      const requested = result.window.requested_outer_rect;
      const actual = result.window.actual_window_rect;
      setWindowActionMessage({
        kind: "ok",
        text: `#${runtimeSessionId} ${result.monitor_device_name} ${result.slot}: 요청 ${requested ? formatRect(requested) : "?"}, 실제 ${actual ? formatRect(actual) : "측정 실패"}`,
      });
      await refresh();
    } catch (actionError) {
      setWindowActionMessage({ kind: "error", text: String(actionError) });
    } finally {
      setWindowActionBusy(null);
    }
  };

  const restoreSingleWindowTest = async (runtimeSessionId: number) => {
    setWindowActionBusy(runtimeSessionId);
    setWindowActionMessage(null);
    try {
      const result = await api.multiclientStage1BRestoreTest(runtimeSessionId);
      const restored = result.window.actual_normal_rect ?? result.window.actual_window_rect;
      setWindowActionMessage({
        kind: "ok",
        text: `#${runtimeSessionId} 원상복구: ${restored ? formatRect(restored) : "복구 후 rect 측정 불가"}`,
      });
      await refresh();
    } catch (actionError) {
      setWindowActionMessage({ kind: "error", text: String(actionError) });
    } finally {
      setWindowActionBusy(null);
    }
  };

  const positionSixWindowsTest = async () => {
    setWindowActionBusy(-1);
    setWindowActionMessage(null);
    try {
      const result = await api.multiclientStage1BPositionSixTest();
      const positionMatches = result.windows.filter(
        ({ window }) => window.requested_position_matches === true,
      ).length;
      const unchangedRects = result.windows.filter(
        ({ window }) => window.requested_outer_rect_matches === true,
      ).length;
      setWindowActionMessage({
        kind: positionMatches === result.windows.length ? "ok" : "error",
        text: `6창 위치 이동: 목표 좌표 ${positionMatches}/${result.windows.length}, 크기까지 불변 ${unchangedRects}/${result.windows.length}`,
      });
      await refresh();
    } catch (actionError) {
      setWindowActionMessage({ kind: "error", text: String(actionError) });
    } finally {
      setWindowActionBusy(null);
    }
  };

  const restoreAllWindowTests = async () => {
    setWindowActionBusy(-1);
    setWindowActionMessage(null);
    try {
      const result = await api.multiclientStage1BRestoreAllTest();
      setWindowActionMessage({
        kind: "ok",
        text: `전체 원상복구: ${result.windows.length}개 창`,
      });
      await refresh();
    } catch (actionError) {
      setWindowActionMessage({ kind: "error", text: String(actionError) });
    } finally {
      setWindowActionBusy(null);
    }
  };

  const disconnectBrokerTest = async (profileId: string, accountId: string) => {
    setBrokerActionBusy(accountId);
    setBrokerActionMessage(null);
    try {
      await api.multiclientStage0DisconnectBrokerTest(profileId, accountId);
      setBrokerActionMessage({
        kind: "ok",
        text: `${accountId}: 연결을 끊었습니다. CE 자동 재연결을 확인합니다.`,
      });
      await refresh();
    } catch (actionError) {
      setBrokerActionMessage({ kind: "error", text: String(actionError) });
    } finally {
      setBrokerActionBusy(null);
    }
  };

  if (!diagnostics?.enabled) return null;

  const registry = diagnostics.registry;
  const activeSessions = registry?.active_sessions ?? [];
  const recentExits = [...(registry?.recent_exits ?? [])].reverse().slice(0, 6);
  const warnings = registry?.warnings ?? [];
  const readyWindowCount = diagnostics.session_windows.filter(
    ({ inspection }) => inspection.management_eligible,
  ).length;
  const sixWindowProofReady = activeSessions.length === 6 && readyWindowCount === 6;
  const anyRestoreAvailable = diagnostics.window_test_restore_available_for.length > 0;

  return (
    <aside className={`stage0-panel ${collapsed ? "is-collapsed" : ""}`}>
      <header className="stage0-panel-header">
        <div>
          <strong>멀티클라 Stage 1B 진단</strong>
          <span className="stage0-live-dot">LIVE</span>
        </div>
        <div className="stage0-panel-actions">
          <button
            type="button"
            onClick={() => void refresh(true)}
            disabled={refreshing}
            title="진단 snapshot 즉시 갱신"
          >
            {refreshing ? "갱신 중" : "새로고침"}
          </button>
          <button
            type="button"
            onClick={() => setCollapsed((value) => !value)}
            title={collapsed ? "진단 패널 펼치기" : "진단 패널 접기"}
          >
            {collapsed ? "펼치기" : "접기"}
          </button>
        </div>
      </header>

      {!collapsed && (
        <div className="stage0-panel-body" aria-live="polite">
          <div className="stage0-summary">
            <span>활성 <b>{activeSessions.length}</b></span>
            <span>게임창 준비 <b>{readyWindowCount}</b></span>
            <span>모니터 <b>{diagnostics.monitors.length}</b></span>
          </div>

          <p className="stage0-help">
            PID의 실제 생성 시각을 재검증하고 같은 프로세스의 Razor Enhanced 창을
            제외한 SDL 게임창만 찾습니다. 아래 테스트 버튼만 명시적으로 창을 변경합니다.
          </p>

          <div className="stage0-window-test-actions">
            <button
              type="button"
              disabled={!sixWindowProofReady || windowActionBusy !== null || anyRestoreAvailable}
              onClick={() => void positionSixWindowsTest()}
              title="#1·#2는 주 모니터 좌/우, #3~#6은 세컨 모니터 2×2 좌표로 이동합니다. 크기는 요청하지 않습니다."
            >
              {windowActionBusy === -1 ? "처리 중" : "6창 위치만 배치"}
            </button>
            {anyRestoreAvailable && (
              <button
                type="button"
                disabled={windowActionBusy !== null}
                onClick={() => void restoreAllWindowTests()}
                title="메모리에 저장된 모든 WINDOWPLACEMENT를 원상복구"
              >
                전체 원상복구
              </button>
            )}
          </div>

          {error && <div className="stage0-error">갱신 실패: {error}</div>}
          {diagnostics.registry_error && (
            <div className="stage0-error">Registry: {diagnostics.registry_error}</div>
          )}
          {diagnostics.monitor_error && (
            <div className="stage0-error">Monitor: {diagnostics.monitor_error}</div>
          )}
          {windowActionMessage && (
            <div className={windowActionMessage.kind === "ok" ? "stage0-action-ok" : "stage0-error"}>
              {windowActionMessage.text}
            </div>
          )}
          {brokerActionMessage && (
            <div className={brokerActionMessage.kind === "ok" ? "stage0-action-ok" : "stage0-error"}>
              {brokerActionMessage.text}
            </div>
          )}

          <section className="stage0-section">
            <h3>CE IPC</h3>
            {diagnostics.broker_sessions.length === 0 ? (
              <div className="stage0-empty">MULTI 실행 후 CE 연결 상태를 표시합니다.</div>
            ) : (
              <div className="stage0-list">
                {diagnostics.broker_sessions.map((session) => (
                  <div className="stage0-window-card" key={session.launch_session_id}>
                    <div className="stage0-window-heading">
                      <div>
                        <b>{session.account_id}</b>
                        <span>PID {session.expected_pid ?? "?"}</span>
                      </div>
                      <span className={`stage0-status ${session.window_ready ? "is-ready" : "is-waiting"}`}>
                        {session.window_ready ? "window_ready" : session.connected ? "연결됨" : "연결 대기"}
                      </span>
                    </div>
                    <code>
                      재연결 {Math.max(0, session.reconnect_count - 1)}회 · {session.managed_tile_active ? "관리 슬롯" : "일반 창"} · HWND {session.hwnd ?? "대기"}
                    </code>
                    {session.pending_apply_layout && (
                      <div className="stage0-warning">재배치 명령 전달 대기</div>
                    )}
                    {session.last_error && (
                      <div className="stage0-error">{session.last_error}</div>
                    )}
                    <div className="stage0-window-test-actions">
                      <button
                        type="button"
                        disabled={!session.connected || brokerActionBusy !== null}
                        onClick={() => void disconnectBrokerTest(session.profile_id, session.account_id)}
                        title="게임과 런처는 유지한 채 Named Pipe만 끊고 CE의 자동 재연결을 검증"
                      >
                        {brokerActionBusy === session.account_id ? "처리 중" : "IPC 재연결 테스트"}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="stage0-section">
            <h3>활성 세션</h3>
            {activeSessions.length === 0 ? (
              <div className="stage0-empty">아직 추적 중인 클라이언트가 없습니다.</div>
            ) : (
              <div className="stage0-list">
                {activeSessions.map((session) => (
                  <div className="stage0-row" key={session.runtime_session_id}>
                    <div>
                      <b>#{session.runtime_session_id}</b>
                      <span>PID {session.pid}</span>
                      <span>{formatTime(session.launcher_observed_spawn_time_unix_ms)}</span>
                    </div>
                    <code title={`${session.profile_id} / ${session.account_id}`}>
                      {session.profile_id} / {session.account_id}
                    </code>
                    {session.process_creation_time_error && (
                      <div className="stage0-error">
                        생성 시각: {session.process_creation_time_error}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </section>

          <section className="stage0-section">
            <h3>PID → 게임 HWND 식별</h3>
            {diagnostics.session_windows.length === 0 ? (
              <div className="stage0-empty">활성 세션을 실행하면 HWND를 조사합니다.</div>
            ) : (
              <div className="stage0-list">
                {diagnostics.session_windows.map((sessionWindow) => {
                  const inspection = sessionWindow.inspection;
                  return (
                    <div className="stage0-window-card" key={sessionWindow.runtime_session_id}>
                      <div className="stage0-window-heading">
                        <div>
                          <b>#{sessionWindow.runtime_session_id}</b>
                          <span>PID {inspection.pid}</span>
                        </div>
                        <span className={`stage0-status ${inspection.management_eligible ? "is-ready" : "is-waiting"}`}>
                          {windowStatusLabel(inspection.status)}
                        </span>
                      </div>
                      <code title={`${sessionWindow.profile_id} / ${sessionWindow.account_id}`}>
                        {sessionWindow.profile_id} / {sessionWindow.account_id}
                      </code>
                      <div className="stage0-identity-check">
                        생성 시각 {inspection.process_creation_time_matches === true ? "일치" : inspection.process_creation_time_matches === false ? "불일치" : "미확인"}
                        {inspection.selected_hwnd && <> · 선택 {inspection.selected_hwnd}</>}
                      </div>
                      <div className="stage0-window-candidates">
                        {inspection.candidates.map((candidate) => {
                          const measuredRect = candidate.client_rect_screen ?? candidate.normal_rect;
                          return (
                            <div className={`stage0-window-candidate ${candidate.eligible_game_window ? "is-selected" : "is-excluded"}`} key={candidate.hwnd}>
                              <div>
                                <b>{candidate.eligible_game_window ? "게임" : "제외"}</b>
                                <span>{candidate.hwnd}</span>
                                <span>{candidate.class_name}</span>
                              </div>
                              <code title={candidate.title}>{candidate.title || "제목 없음"}</code>
                              <code>
                                DPI {candidate.dpi ?? "?"} · {candidate.is_minimized ? "최소화" : "표시"}
                                {measuredRect ? ` · ${formatRect(measuredRect)}` : " · 측정 가능한 rect 없음"}
                              </code>
                            </div>
                          );
                        })}
                      </div>
                      <div className="stage0-window-test-actions">
                        <button
                          type="button"
                          disabled={!inspection.management_eligible || windowActionBusy !== null}
                          onClick={() => void moveSingleWindowTest(sessionWindow.runtime_session_id)}
                          title="이 게임창 한 개의 현재 배치를 메모리에 보관하고 세컨 모니터 좌상단 셀로 이동"
                        >
                          {windowActionBusy === sessionWindow.runtime_session_id ? "처리 중" : "r0c0 단일창 이동"}
                        </button>
                        {diagnostics.window_test_restore_available_for.includes(sessionWindow.runtime_session_id) && (
                          <button
                            type="button"
                            disabled={windowActionBusy !== null}
                            onClick={() => void restoreSingleWindowTest(sessionWindow.runtime_session_id)}
                            title="테스트 직전에 저장한 WINDOWPLACEMENT로 원상복구"
                          >
                            원상복구
                          </button>
                        )}
                      </div>
                      {inspection.warnings.map((warning, index) => (
                        <div className="stage0-warning" key={`${sessionWindow.runtime_session_id}-${index}`}>
                          {warning}
                        </div>
                      ))}
                    </div>
                  );
                })}
              </div>
            )}
          </section>

          {(warnings.length > 0 || (registry?.untracked_launches.length ?? 0) > 0) && (
            <section className="stage0-section">
              <h3>경고</h3>
              {warnings.map((warning, index) => (
                <div className="stage0-warning" key={`${warning.code}-${index}`}>
                  <b>{warning.code}</b>: {warning.message}
                </div>
              ))}
              {registry?.untracked_launches.map((launch, index) => (
                <div className="stage0-warning" key={`${launch.code}-${index}`}>
                  <b>{launch.code}</b>: {launch.profile_id} / {launch.account_id ?? "account 없음"}
                </div>
              ))}
            </section>
          )}

          <section className="stage0-section">
            <h3>모니터와 2×2 dry-run</h3>
            <div className="stage0-list">
              {diagnostics.monitors.map(({ monitor, cells, layout_error }) => (
                <div className="stage0-monitor" key={monitor.device_name}>
                  <div>
                    <b>{monitor.device_name}</b>
                    {monitor.is_primary && <span className="stage0-primary">주 모니터</span>}
                  </div>
                  <code>work {formatRect(monitor.work_area)}</code>
                  {layout_error ? (
                    <div className="stage0-error">{layout_error}</div>
                  ) : (
                    <div className="stage0-cells">
                      {cells?.map((cell) => (
                        <code key={cell.slot}>{cell.slot} {formatRect(cell.rect)}</code>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </section>

          {recentExits.length > 0 && (
            <section className="stage0-section">
              <h3>최근 종료</h3>
              <div className="stage0-list">
                {recentExits.map((session) => (
                  <div className="stage0-row" key={session.runtime_session_id}>
                    <div>
                      <b>#{session.runtime_session_id}</b>
                      <span>PID {session.pid}</span>
                      <span>exit {session.exit_code ?? "?"}</span>
                    </div>
                    <code>{session.profile_id} / {session.account_id}</code>
                  </div>
                ))}
              </div>
            </section>
          )}

          <details className="stage0-raw">
            <summary>원본 JSON</summary>
            <pre>{JSON.stringify(diagnostics, null, 2)}</pre>
          </details>
        </div>
      )}
    </aside>
  );
}
