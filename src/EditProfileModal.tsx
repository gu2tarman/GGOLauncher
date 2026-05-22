import { useEffect, useState } from "react";
import { Modal } from "./Modal";
import { api } from "./api";
import type { EncryptionType, PathInfo, Profile } from "./types";

type Props = {
  open: boolean;
  profile: Profile | null;
  onClose: () => void;
  onSave: (next: Profile) => void;
};

const ENCRYPTIONS: { value: EncryptionType; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "none", label: "None" },
  { value: "old_blowfish", label: "Old Blowfish" },
  { value: "blowfish125", label: "1.25.36+" },
  { value: "twofish", label: "Twofish" },
];

export function EditProfileModal({ open, profile, onClose, onSave }: Props) {
  // 로컬 폼 state — 모달 열릴 때마다 prop으로 초기화. Save 시에만 부모에 반영.
  const [draft, setDraft] = useState<Profile | null>(profile);
  const [showPw, setShowPw] = useState(false);
  const [uoCheck, setUoCheck] = useState<PathInfo | null>(null);
  const [cuoCheck, setCuoCheck] = useState<PathInfo | null>(null);

  useEffect(() => {
    setDraft(profile);
    setShowPw(false);
  }, [profile, open]);

  // 경로 입력 변경 시 디바운스로 inspect_path 호출
  useEffect(() => {
    if (!draft?.uo_path) {
      setUoCheck(null);
      return;
    }
    const t = setTimeout(() => {
      api.inspectPath(draft.uo_path).then(setUoCheck).catch(console.error);
    }, 300);
    return () => clearTimeout(t);
  }, [draft?.uo_path]);

  useEffect(() => {
    if (!draft?.cuo_path) {
      setCuoCheck(null);
      return;
    }
    const t = setTimeout(() => {
      api.inspectPath(draft.cuo_path!).then(setCuoCheck).catch(console.error);
    }, 300);
    return () => clearTimeout(t);
  }, [draft?.cuo_path]);

  if (!draft) return null;

  const update = <K extends keyof Profile>(key: K, value: Profile[K]) =>
    setDraft((d) => (d ? { ...d, [key]: value } : d));

  const updateServer = <K extends keyof Profile["server"]>(
    key: K,
    value: Profile["server"][K]
  ) =>
    setDraft((d) =>
      d ? { ...d, server: { ...d.server, [key]: value } } : d
    );

  const pickUo = async () => {
    const picked = await api.clientSelectDirectory(draft.uo_path || undefined);
    if (picked) update("uo_path", picked);
  };
  const pickCuo = async () => {
    const picked = await api.cuoSelectDirectory(draft.cuo_path || undefined);
    if (picked) update("cuo_path", picked);
  };

  const canSave = draft.name.trim().length > 0 && draft.server.address.trim().length > 0;

  const onSaveClick = () => {
    if (!canSave || !draft) return;
    onSave(draft);
    onClose();
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="프로필 편집"
      width={680}
      headerActions={
        <>
          <button className="btn-action" onClick={onClose}>취소</button>
          <button
            className="btn-primary btn-primary-sm"
            onClick={onSaveClick}
            disabled={!canSave}
          >
            저장
          </button>
        </>
      }
    >
      {/* Profile Info */}
      <section className="form-section">
        <header className="form-section-title">프로필 정보</header>
        <Field label="이름">
          <input
            className="text-input"
            value={draft.name}
            onChange={(e) => update("name", e.target.value)}
            placeholder="예: 메인 캐릭터"
          />
        </Field>
      </section>

      {/* Game Paths */}
      <section className="form-section">
        <header className="form-section-title">게임 경로</header>
        <Field label="UO 경로" hint="Ultima Online 클라이언트 폴더">
          <PathRow
            value={draft.uo_path}
            onChange={(v) => update("uo_path", v)}
            onPick={pickUo}
            check={uoCheck}
            placeholder="C:\Ultima Online"
            validateKind="uo"
          />
        </Field>
        <Field label="CUO 경로" hint="ClassicUO.exe가 있는 폴더 (선택)">
          <PathRow
            value={draft.cuo_path ?? ""}
            onChange={(v) => update("cuo_path", v || null)}
            onPick={pickCuo}
            check={cuoCheck}
            placeholder="(선택)"
            validateKind="cuo"
          />
        </Field>
        <Field label="클라이언트 버전" hint="예: 7.0.95.0 (비워두면 자동)">
          <input
            className="text-input"
            value={draft.client_version ?? ""}
            onChange={(e) => update("client_version", e.target.value || null)}
            placeholder="(자동 감지)"
          />
        </Field>
      </section>

      {/* Server */}
      <section className="form-section">
        <header className="form-section-title">서버 접속</header>
        <Field label="계정">
          <input
            className="text-input"
            value={draft.server.username}
            onChange={(e) => updateServer("username", e.target.value)}
            autoComplete="off"
          />
        </Field>
        <Field label="비밀번호" hint="저장 시 암호화됩니다 (예정)">
          <div className="password-row">
            <input
              className="text-input"
              type={showPw ? "text" : "password"}
              value={draft.server.password_encrypted}
              onChange={(e) => updateServer("password_encrypted", e.target.value)}
              autoComplete="new-password"
            />
            <button
              type="button"
              className="btn-action"
              onClick={() => setShowPw((s) => !s)}
            >
              {showPw ? "숨기기" : "보이기"}
            </button>
          </div>
        </Field>
        <div className="row-2">
          <Field label="서버 주소">
            <input
              className="text-input"
              value={draft.server.address}
              onChange={(e) => updateServer("address", e.target.value)}
              placeholder="login.uoserver.com"
            />
          </Field>
          <Field label="포트" width={120}>
            <input
              className="text-input"
              type="number"
              value={draft.server.port}
              onChange={(e) =>
                updateServer("port", Number(e.target.value) || 0)
              }
              min={1}
              max={65535}
            />
          </Field>
        </div>
        <Field label="암호화 타입">
          <select
            className="text-input"
            value={draft.server.encryption}
            onChange={(e) =>
              updateServer("encryption", e.target.value as EncryptionType)
            }
          >
            {ENCRYPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </Field>
      </section>
    </Modal>
  );
}

// ── 소형 컴포넌트 ─────────────────────────────────────────
function Field({
  label,
  hint,
  width,
  children,
}: {
  label: string;
  hint?: string;
  width?: number;
  children: React.ReactNode;
}) {
  return (
    <div className="form-field" style={width ? { width } : undefined}>
      <label className="form-label">
        {label}
        {hint && <span className="form-hint">{hint}</span>}
      </label>
      {children}
    </div>
  );
}

function PathRow({
  value,
  onChange,
  onPick,
  check,
  placeholder,
  validateKind,
}: {
  value: string;
  onChange: (v: string) => void;
  onPick: () => void;
  check: PathInfo | null;
  placeholder?: string;
  validateKind: "uo" | "cuo";
}) {
  let mark: React.ReactNode = null;
  if (value && check) {
    const ok = validateKind === "uo" ? check.valid_uo : check.valid_cuo;
    if (ok) {
      mark = <span className="path-mark ok">✓</span>;
    } else if (check.exists) {
      mark = <span className="path-mark warn" title="폴더는 있지만 유효하지 않은 듯">!</span>;
    } else {
      mark = <span className="path-mark err">✗</span>;
    }
  }
  return (
    <div className="path-row">
      <input
        className="text-input path-input"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
      />
      {mark}
      <button type="button" className="btn-action" onClick={onPick}>
        찾기
      </button>
    </div>
  );
}
