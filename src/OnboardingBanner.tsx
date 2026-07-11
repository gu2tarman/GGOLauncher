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

  // 3단계 다 채워졌으면 자동 dismiss (부모가 done===3 감지해서 onDismiss 호출)

  const Step = ({ ok, label, hint }: { ok: boolean; label: string; hint: string }) => (
    <div className={`onboarding-step ${ok ? "is-done" : ""}`}>
      <span className="onboarding-check">{ok ? "✓" : "○"}</span>
      <span className="onboarding-label">{label}</span>
      <span className="onboarding-hint">→ {hint}</span>
    </div>
  );

  return (
    <div className="onboarding-banner" role="region" aria-label="첫 사용 가이드">
      <div className="onboarding-head">
        <span className="onboarding-title">시작하기 ({done}/3)</span>
        <button
          className="onboarding-close"
          onClick={onDismiss}
          aria-label="가이드 닫기"
          title="가이드 닫기 (다시 표시되지 않습니다)"
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
