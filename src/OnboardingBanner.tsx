import type { Settings } from "./types";

type Props = {
  settings: Settings;
  /** 닫기 클릭 시 settings.ui.onboarding_dismissed=true 영구 저장 */
  onDismiss: () => void;
};

/**
 * 첫 사용 온보딩 배너.
 * 3단계 체크리스트 — 자동 판정 + 사용자 X 닫기 둘 다 dismiss 처리.
 * 이미 dismissed면 렌더 안 함.
 */
export function OnboardingBanner({ settings, onDismiss }: Props) {
  if (settings.ui?.onboarding_dismissed) return null;

  const hasPlugin = settings.plugins.length > 0;
  const hasProfile = settings.profiles.length > 0;
  const hasLaunched = !!settings.ui?.first_launch_completed;
  const done = [hasPlugin, hasProfile, hasLaunched].filter(Boolean).length;

  // 3단계 다 채워졌으면 자동 dismiss (effect로 위임)
  // 여기서는 표시만 하고, 부모가 done===3 감지해서 onDismiss 호출하도록 함

  const Step = ({ ok, label, hint }: { ok: boolean; label: string; hint: string }) => (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        gap: 8,
        padding: "2px 0",
        opacity: ok ? 0.55 : 1,
      }}
    >
      <span
        style={{
          width: 16,
          textAlign: "center",
          color: ok ? "#5ad17e" : "#888",
          fontSize: 13,
        }}
      >
        {ok ? "✓" : "☐"}
      </span>
      <span style={{ fontWeight: 600 }}>{label}</span>
      <span style={{ opacity: 0.7, fontSize: 11 }}>→ {hint}</span>
    </div>
  );

  return (
    <div
      role="region"
      aria-label="첫 사용 가이드"
      style={{
        background: "rgba(110, 160, 220, 0.08)",
        border: "1px solid rgba(110, 160, 220, 0.3)",
        borderRadius: 8,
        padding: "10px 14px",
        marginBottom: 10,
        fontSize: 12,
        position: "relative",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 6,
        }}
      >
        <span style={{ fontWeight: 700, fontSize: 13 }}>
          🎯 시작하기 ({done}/3)
        </span>
        <button
          onClick={onDismiss}
          aria-label="가이드 닫기"
          title="가이드 닫기 (다시 표시되지 않습니다)"
          style={{
            background: "transparent",
            border: "none",
            color: "inherit",
            cursor: "pointer",
            opacity: 0.6,
            fontSize: 14,
            padding: "0 4px",
            lineHeight: 1,
          }}
        >
          ✕
        </button>
      </div>
      <Step ok={hasPlugin} label="1. 플러그인 등록" hint="Razor 또는 ClassicAssist" />
      <Step ok={hasProfile} label="2. 프로필 생성" hint="프로필 관리에서 첫 프로필 생성" />
      <Step ok={hasLaunched} label="3. PLAY 또는 MULTI LOGIN" hint="실행해보면 완료" />
    </div>
  );
}
