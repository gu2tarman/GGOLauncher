import { invoke } from "@tauri-apps/api/core";

type LinkButtonProps = {
  label: string;
  url?: string;
};

function LinkButton({ label, url }: LinkButtonProps) {
  const disabled = !url;
  const onClick = () => {
    if (!url) return;
    invoke("open_external", { url }).catch((e) => console.error(e));
  };
  return (
    <button
      className={`side-btn ${disabled ? "side-btn-disabled" : ""}`}
      onClick={onClick}
      disabled={disabled}
      title={disabled ? "준비 중" : url}
    >
      {label}
      {disabled && <span className="badge-soon">준비중</span>}
    </button>
  );
}

function App() {
  return (
    <div className="app">
      {/* ── Left column ─────────────────────────── */}
      <aside className="left-column">
        <div className="logo-wrap">
          <img src="/margo-logo.png" alt="Margo Launcher" className="logo" />
          <div className="fan-made-note">* Unofficial fan-made launcher</div>
        </div>

        <div className="btn-group">
          <div className="btn-group-label">MARGO</div>
          <LinkButton label="Discord" url="https://discord.gg/VGfYrJFXtH" />
          <LinkButton label="Website" />
          <LinkButton label="오픈카톡" />
        </div>

        <div className="btn-group">
          <div className="btn-group-label">GGO SUPPORT</div>
          <LinkButton label="웹훅 발급소" url="https://discord.gg/KQzHZsZ9eH" />
          <LinkButton label="문의하기" url="https://open.kakao.com/o/sA71kz5d" />
        </div>

        <div className="btn-group">
          <div className="btn-group-label">ORIGINAL CLASSICUO</div>
          <LinkButton label="클래식유오" url="https://www.classicuo.eu" />
        </div>
      </aside>

      {/* ── Right column ────────────────────────── */}
      <main className="right-column">
        <div className="top-actions">
          <button className="btn-play">PLAY</button>
          <button className="btn-update btn-update-checking" disabled>
            업데이트 확인 중...
          </button>
        </div>

        <section className="profile-box">
          <header className="profile-header">
            <span className="profile-title">DESKTOP CLIENT SETTINGS</span>
            <span className="profile-version">ClassicUO v—</span>
          </header>
          <div className="profile-body">
            <div className="profile-info">
              <div className="profile-name">프로필 없음</div>
              <div className="profile-addr">—</div>
            </div>
            <button className="btn-change">Change</button>
          </div>
        </section>

        <section className="plugin-panel">
          <header className="panel-header panel-header-row">
            <span>
              활성 플러그인 <span className="panel-subnote">(공용)</span>
            </span>
            <div className="panel-actions">
              <button className="btn-small" onClick={() => console.log("Add plugin (Phase 5)")}>
                + Add
              </button>
              <button className="btn-small" onClick={() => console.log("Import ZIP (Phase 5)")}>
                Import ZIP
              </button>
            </div>
          </header>
          <div className="plugin-grid">
            <div className="plugin-empty">등록된 플러그인이 없습니다</div>
          </div>
        </section>

        <section className="notice-row">
          <div className="notice-card">
            <header className="panel-header">📢 Margo 공지</header>
            <div className="notice-placeholder">불러오는 중...</div>
          </div>
          <div className="notice-card">
            <header className="panel-header">📢 GGOUO 공지</header>
            <div className="notice-placeholder">불러오는 중...</div>
          </div>
        </section>
      </main>
    </div>
  );
}

export default App;
