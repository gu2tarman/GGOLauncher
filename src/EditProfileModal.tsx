import { type PointerEvent, useEffect, useMemo, useRef, useState } from "react";
import { Modal } from "./Modal";
import { api } from "./api";
import type {
  Account,
  CuoProfileCandidate,
  EncryptionType,
  PathInfo,
  Profile,
  SecondaryLayoutPreset,
} from "./types";

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

type SecondarySlot = NonNullable<Account["secondary_slot"]>;
type LayoutDropTarget = SecondarySlot | "unassigned";

type LayoutPointerGesture = {
  accountId: string;
  pointerId: number;
  sourceSlot: SecondarySlot | null;
  startX: number;
  startY: number;
  moved: boolean;
};

const BASE_LAYOUT_SLOTS: SecondarySlot[] = ["r0c0", "r0c1", "r1c0", "r1c1"];
const CENTER_LAYOUT_SLOTS: SecondarySlot[] = [...BASE_LAYOUT_SLOTS, "center"];

const SLOT_LABELS: Record<SecondarySlot, string> = {
  r0c0: "왼쪽 위",
  r0c1: "오른쪽 위",
  r1c0: "왼쪽 아래",
  r1c1: "오른쪽 아래",
  center: "중앙",
};

export function EditProfileModal({ open, profile, onClose, onSave }: Props) {
  // 로컬 폼 state — 모달 열릴 때마다 prop으로 초기화. Save 시에만 부모에 반영.
  const [draft, setDraft] = useState<Profile | null>(profile);
  const [uoCheck, setUoCheck] = useState<PathInfo | null>(null);
  const [cuoCheck, setCuoCheck] = useState<PathInfo | null>(null);
  const [autoVersion, setAutoVersion] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  // 각 계정 비밀번호 show/hide 상태 (id → bool)
  const [showPw, setShowPw] = useState<Record<string, boolean>>({});
  const [saveError, setSaveError] = useState<string | null>(null);
  const [multiWarning, setMultiWarning] = useState<string | null>(null);
  const [cuoProfileCandidates, setCuoProfileCandidates] = useState<CuoProfileCandidate[]>([]);
  const [draggedAccountId, setDraggedAccountId] = useState<string | null>(null);
  const [dragOverTarget, setDragOverTarget] = useState<LayoutDropTarget | null>(null);
  const layoutPointerGesture = useRef<LayoutPointerGesture | null>(null);

  // multiWarning 3초 자동 닫힘
  useEffect(() => {
    if (!multiWarning) return;
    const t = setTimeout(() => setMultiWarning(null), 3000);
    return () => clearTimeout(t);
  }, [multiWarning]);

  useEffect(() => {
    if (!profile) {
      setDraft(null);
      setShowPw({});
      setDraggedAccountId(null);
      setDragOverTarget(null);
      layoutPointerGesture.current = null;
      return;
    }
    // 비밀번호 복호화 — draft에는 평문 보관. 저장 시 다시 암호화.
    let cancelled = false;
    (async () => {
      const accounts = await Promise.all(
        profile.server.accounts.map(async (a) => ({
          ...a,
          password_encrypted: a.password_encrypted
            ? await api.decryptPassword(a.password_encrypted).catch(() => "")
            : "",
        }))
      );
      if (cancelled) return;
      setDraft({ ...profile, server: { ...profile.server, accounts } });
      setShowPw({});
      setDraggedAccountId(null);
      setDragOverTarget(null);
      layoutPointerGesture.current = null;
    })();
    return () => {
      cancelled = true;
    };
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
      setCuoProfileCandidates([]);
      return;
    }
    const t = setTimeout(() => {
      api.inspectPath(draft.cuo_path!).then(setCuoCheck).catch(console.error);
    }, 300);
    return () => clearTimeout(t);
  }, [draft?.cuo_path]);

  useEffect(() => {
    const cuoPath = draft?.cuo_path?.trim();
    if (!cuoPath) {
      setCuoProfileCandidates([]);
      return;
    }
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .listCuoProfiles(cuoPath)
        .then((items) => {
          if (!cancelled) setCuoProfileCandidates(items);
        })
        .catch((e) => {
          console.warn("[cuo profiles]", e);
          if (!cancelled) setCuoProfileCandidates([]);
        });
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [draft?.cuo_path]);

  // UO 경로 유효해지면 client.exe 버전 자동 감지
  useEffect(() => {
    setAutoVersion(null);
    if (!uoCheck?.valid_uo || !draft?.uo_path) return;
    let cancelled = false;
    setDetecting(true);
    api
      .detectClientVersion(draft.uo_path)
      .then((v) => {
        if (!cancelled) setAutoVersion(v);
      })
      .catch(console.error)
      .finally(() => !cancelled && setDetecting(false));
    return () => {
      cancelled = true;
    };
  }, [uoCheck?.valid_uo, draft?.uo_path]);

  const accountCandidateValues = useMemo(
    () => cuoProfileCandidates.map((c) => c.account),
    [cuoProfileCandidates]
  );

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

  const updateAccount = (id: string, patch: Partial<Account>) =>
    setDraft((d) =>
      d
        ? {
            ...d,
            server: {
              ...d.server,
              accounts: d.server.accounts.map((a) =>
                a.id === id ? { ...a, ...patch } : a
              ),
            },
          }
        : d
    );

  // 계정의 MULTI 포함 여부 해석 (레거시: 인덱스<6이면 true).
  const isMultiEnabled = (a: Account, i: number) =>
    a.multi_enabled == null ? i < 6 : a.multi_enabled === true;

  const addAccount = () =>
    setDraft((d) => {
      if (!d) return d;
      const id = `a_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
      // 비번 자동 채움: 기존 계정 중 비밀번호가 있는 첫 항목에서 복사.
      // (보통 같은 서버 다중 캐릭은 비번 공유 — 다르면 사용자가 그 자리에서 수정)
      const defaultPw =
        d.server.accounts.find((a) => a.password_encrypted.trim().length > 0)
          ?.password_encrypted ?? "";
      // 신규 계정의 multi_enabled 기본값:
      //   - 현재 계정 수가 6 미만이면 true (자동 포함)
      //   - 6 이상이면 false (사용자가 명시적으로 선택)
      const currentCount = d.server.accounts.length;
      const newAcc: Account = {
        id,
        username: "",
        password_encrypted: defaultPw,
        character_name: "",
        multi_enabled: currentCount < 6,
        secondary_slot: null,
      };
      return {
        ...d,
        server: {
          ...d.server,
          accounts: [...d.server.accounts, newAcc],
          active_account_id: d.server.active_account_id ?? id,
        },
      };
    });

  // MULTI 체크박스 토글 — 최대 6개 cap.
  const toggleMulti = (id: string, want: boolean) => {
    setDraft((d) => {
      if (!d) return d;
      // 현재 체크된 수 계산 (레거시 정책 포함)
      const currentCount = d.server.accounts.filter((a, i) =>
        isMultiEnabled(a, i)
      ).length;
      if (want && currentCount >= 6) {
        setMultiWarning("MULTI LOGIN은 최대 6개까지만 선택할 수 있습니다.");
        return d;
      }
      setMultiWarning(null);
      let accounts = d.server.accounts.map((a) =>
        a.id === id
          ? { ...a, multi_enabled: want, secondary_slot: want ? a.secondary_slot : null }
          : a
      );
      const nextLeader = accounts.find((a, index) => isMultiEnabled(a, index));
      if (nextLeader?.secondary_slot != null) {
        accounts = accounts.map((a) =>
          a.id === nextLeader.id ? { ...a, secondary_slot: null } : a
        );
      }
      return {
        ...d,
        server: {
          ...d.server,
          accounts,
        },
      };
    });
  };

  const removeAccount = (id: string) =>
    setDraft((d) => {
      if (!d) return d;
      const accounts = d.server.accounts.filter((a) => a.id !== id);
      const active =
        d.server.active_account_id === id
          ? accounts[0]?.id ?? null
          : d.server.active_account_id;
      return {
        ...d,
        server: { ...d.server, accounts, active_account_id: active },
      };
    });

  const pickUo = async () => {
    const picked = await api.clientSelectDirectory(draft.uo_path || undefined);
    if (picked) update("uo_path", picked);
  };
  const pickCuo = async () => {
    const picked = await api.cuoSelectDirectory(draft.cuo_path || undefined);
    if (picked) update("cuo_path", picked);
  };

  const canSave = draft.name.trim().length > 0 && draft.server.address.trim().length > 0;
  const characterCandidatesFor = (accountName: string) => {
    const normalized = accountName.trim().toLowerCase();
    if (!normalized) {
      return uniqueSorted(cuoProfileCandidates.flatMap((c) => c.characters));
    }
    const matched = cuoProfileCandidates.find(
      (c) => c.account.toLowerCase() === normalized
    );
    return matched?.characters ?? [];
  };

  const layoutPreset: SecondaryLayoutPreset =
    draft.secondary_layout_preset ?? "two_by_two";
  const layoutSlots =
    layoutPreset === "two_by_two_center" ? CENTER_LAYOUT_SLOTS : BASE_LAYOUT_SLOTS;
  const selectedForMulti = draft.server.accounts
    .filter((a, i) => isMultiEnabled(a, i))
    .slice(0, 6);
  const leaderAccountId = selectedForMulti[0]?.id ?? null;
  const secondaryAccounts = selectedForMulti.filter((a) => a.id !== leaderAccountId);
  const unassignedSecondaryAccounts = secondaryAccounts.filter(
    (account) => account.secondary_slot == null || !layoutSlots.includes(account.secondary_slot)
  );

  const accountLabel = (account: Account) =>
    account.character_name?.trim() ||
    account.display_name?.trim() ||
    account.username.trim() ||
    "이름 없는 계정";

  const setLayoutPreset = (preset: SecondaryLayoutPreset) => {
    setDraft((d) => {
      if (!d) return d;
      const accounts = d.server.accounts.map((account) =>
        preset === "two_by_two" && account.secondary_slot === "center"
          ? { ...account, secondary_slot: null }
          : account
      );
      return {
        ...d,
        secondary_layout_preset: preset,
        server: { ...d.server, accounts },
      };
    });
  };

  const moveAccountToSlot = (accountId: string, slot: SecondarySlot | null) => {
    setDraft((d) => {
      if (!d) return d;
      const selected = d.server.accounts
        .filter((account, index) => isMultiEnabled(account, index))
        .slice(0, 6);
      const leaderId = selected[0]?.id;
      if (accountId === leaderId || !selected.some((account) => account.id === accountId))
        return d;

      const validSlots =
        d.secondary_layout_preset === "two_by_two_center"
          ? CENTER_LAYOUT_SLOTS
          : BASE_LAYOUT_SLOTS;
      if (slot != null && !validSlots.includes(slot)) return d;

      const moving = d.server.accounts.find((account) => account.id === accountId);
      if (!moving) return d;
      const previousSlot =
        moving.secondary_slot != null && validSlots.includes(moving.secondary_slot)
          ? moving.secondary_slot
          : null;
      const displaced =
        slot == null
          ? null
          : selected.find(
              (account) => account.id !== accountId && account.secondary_slot === slot
            );

      const accounts = d.server.accounts.map((account) => {
        if (account.id === accountId) return { ...account, secondary_slot: slot };
        if (displaced && account.id === displaced.id)
          return { ...account, secondary_slot: previousSlot };
        return account;
      });
      return { ...d, server: { ...d.server, accounts } };
    });
  };

  const layoutDropTargetAt = (clientX: number, clientY: number) => {
    const element = document
      .elementFromPoint(clientX, clientY)
      ?.closest<HTMLElement>("[data-layout-drop]");
    const value = element?.dataset.layoutDrop;
    if (value === "unassigned") return value;
    if (value && layoutSlots.includes(value as SecondarySlot)) return value as SecondarySlot;
    return null;
  };

  const clearAccountPointerDrag = () => {
    layoutPointerGesture.current = null;
    setDraggedAccountId(null);
    setDragOverTarget(null);
  };

  const beginAccountPointerDrag = (
    event: PointerEvent<HTMLButtonElement>,
    accountId: string,
    sourceSlot: SecondarySlot | null
  ) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    layoutPointerGesture.current = {
      accountId,
      pointerId: event.pointerId,
      sourceSlot,
      startX: event.clientX,
      startY: event.clientY,
      moved: false,
    };
    setDraggedAccountId(accountId);
    setDragOverTarget(null);
  };

  const updateAccountPointerDrag = (event: PointerEvent<HTMLButtonElement>) => {
    const gesture = layoutPointerGesture.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (
      !gesture.moved &&
      Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) < 5
    )
      return;

    gesture.moved = true;
    event.preventDefault();
    setDragOverTarget(layoutDropTargetAt(event.clientX, event.clientY));
  };

  const finishAccountPointerDrag = (event: PointerEvent<HTMLButtonElement>) => {
    const gesture = layoutPointerGesture.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    const target = gesture.moved
      ? layoutDropTargetAt(event.clientX, event.clientY)
      : null;
    if (event.currentTarget.hasPointerCapture(event.pointerId))
      event.currentTarget.releasePointerCapture(event.pointerId);
    clearAccountPointerDrag();

    if (gesture.moved && target != null) {
      moveAccountToSlot(gesture.accountId, target === "unassigned" ? null : target);
    } else if (!gesture.moved && gesture.sourceSlot != null) {
      moveAccountToSlot(gesture.accountId, null);
    }
  };

  const cancelAccountPointerDrag = (event: PointerEvent<HTMLButtonElement>) => {
    const gesture = layoutPointerGesture.current;
    if (gesture?.pointerId !== event.pointerId) return;
    clearAccountPointerDrag();
  };

  const onSaveClick = async () => {
    if (!canSave || !draft) return;
    setSaveError(null);
    const selectedForSave = draft.server.accounts
      .filter((a, i) => isMultiEnabled(a, i))
      .slice(0, 6);
    const saveLeaderId = selectedForSave[0]?.id ?? null;
    const assignedSlots = selectedForSave
      .filter((account) => account.id !== saveLeaderId)
      .map((a) => a.secondary_slot)
      .filter((slot): slot is NonNullable<Account["secondary_slot"]> => slot != null);
    const duplicateSlot = assignedSlots.find(
      (slot, index) => assignedSlots.indexOf(slot) !== index
    );
    if (duplicateSlot) {
      setSaveError(`세컨 모니터 슬롯 ${duplicateSlot}이(가) 중복 지정되었습니다.`);
      return;
    }
    // 저장 직전 — draft의 평문 비밀번호를 다시 암호화.
    // 암호화 실패 시 저장 중단(평문 저장 금지).
    try {
      let multiSeen = 0;
      const accounts = await Promise.all(
        draft.server.accounts.map(async (a, i) => {
          const wantsMulti = isMultiEnabled(a, i);
          const multi_enabled = wantsMulti && multiSeen < 6;
          if (wantsMulti) multiSeen += 1;
          return {
            ...a,
            multi_enabled,
            secondary_slot:
              multi_enabled && a.id !== saveLeaderId ? a.secondary_slot : null,
            character_name: a.character_name?.trim() || null,
            password_encrypted: a.password_encrypted
              ? await api.encryptPassword(a.password_encrypted)
              : "",
          };
        })
      );
      const encrypted: Profile = {
        ...draft,
        server: { ...draft.server, accounts },
      };
      onSave(encrypted);
      onClose();
    } catch (e) {
      setSaveError(
        `비밀번호 암호화 실패로 저장이 중단되었습니다. (평문 저장 차단)\n${String(e)}`
      );
    }
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
      {saveError && (
        <div
          className="error-toast"
          style={{ marginBottom: 12, whiteSpace: "pre-wrap" }}
          onClick={() => setSaveError(null)}
        >
          {saveError}
        </div>
      )}

      {cuoProfileCandidates.length > 0 && (
        <datalist id="cuo-account-candidates">
          {accountCandidateValues.map((account) => (
            <option key={account} value={account} />
          ))}
        </datalist>
      )}

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
        <Field
          label="클라이언트 버전"
          hint={
            detecting
              ? "감지 중..."
              : autoVersion
              ? `자동 감지된 값입니다 (필요 시만 수정)`
              : "client.exe에서 자동 감지 (UO 경로 먼저 설정)"
          }
        >
          <input
            className="text-input"
            value={draft.client_version ?? autoVersion ?? ""}
            onChange={(e) => update("client_version", e.target.value || null)}
            placeholder="(자동 감지)"
          />
        </Field>
      </section>

      {/* Server */}
      <section className="form-section">
        <header className="form-section-title">서버 접속</header>
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

      {/* Accounts */}
      <section className="form-section">
        <header className="form-section-title account-header">
          <span>
            계정 목록 <span className="form-hint">(다중 캐릭터 연속 로그인용)</span>
          </span>
          <button
            type="button"
            className="btn-action"
            onClick={addAccount}
            title="이전 계정의 비밀번호가 자동 채워집니다 (수정 가능)"
          >
            + 계정 추가
          </button>
        </header>
        {draft.server.accounts.length === 0 && (
          <div
            className="account-empty"
            style={{
              border: "1px dashed rgba(255,255,255,0.18)",
              borderRadius: 6,
              padding: "14px 12px",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 8,
              textAlign: "center",
            }}
          >
            <div style={{ fontSize: 20, lineHeight: 1 }}>👤</div>
            <div style={{ fontWeight: 600 }}>등록된 계정이 없습니다</div>
            <div style={{ fontSize: 12, opacity: 0.8 }}>
              MULTI LOGIN을 사용하려면 계정 추가가 필수입니다.<br />
              (PLAY 단독은 CUO 로그인 화면에서 직접 입력 가능)
            </div>
            <button
              type="button"
              className="btn-primary btn-primary-sm"
              onClick={addAccount}
              title="이 프로필에 첫 계정 추가"
              style={{ marginTop: 4 }}
            >
              + 첫 계정 추가
            </button>
          </div>
        )}
        {draft.server.accounts.length > 0 && (
          <div style={{ fontSize: 12, opacity: 0.7, marginBottom: 6 }}>
            MULTI LOGIN 대상: {draft.server.accounts.filter((a, i) => isMultiEnabled(a, i)).slice(0, 6).length}/6 선택됨
            {multiWarning && (
              <span style={{ color: "#f88", marginLeft: 8 }}>{multiWarning}</span>
            )}
          </div>
        )}
        {draft.server.accounts.map((acc, i) => {
          const reveal = !!showPw[acc.id];
          const multiOn = isMultiEnabled(acc, i);
          return (
            <div key={acc.id} className="account-row">
              <label
                className="account-radio"
                title={
                  multiOn
                    ? "MULTI 포함됨 — 해제하려면 클릭 (실행 대상에서 제외)"
                    : "MULTI 제외됨 — 클릭하여 포함 (최대 6개까지)"
                }
              >
                <input
                  type="checkbox"
                  checked={multiOn}
                  onChange={(e) => toggleMulti(acc.id, e.target.checked)}
                  aria-label={multiOn ? "MULTI 포함 해제" : "MULTI 포함"}
                />
              </label>
              <div className="account-fields">
                <input
                  className="text-input account-username"
                  placeholder="계정 입력/선택"
                  value={acc.username}
                  list="cuo-account-candidates"
                  onChange={(e) =>
                    updateAccount(acc.id, { username: e.target.value })
                  }
                  autoComplete="off"
                />
                <div className="account-pw">
                  <input
                    className="text-input"
                    type={reveal ? "text" : "password"}
                    placeholder="비밀번호"
                    value={acc.password_encrypted}
                    onChange={(e) =>
                      updateAccount(acc.id, { password_encrypted: e.target.value })
                    }
                    autoComplete="new-password"
                  />
                  <button
                    type="button"
                    className="btn-action"
                    onClick={() =>
                      setShowPw((s) => ({ ...s, [acc.id]: !s[acc.id] }))
                    }
                  >
                    {reveal ? "숨기기" : "보기"}
                  </button>
                </div>
                <input
                  className="text-input account-character"
                  placeholder="캐릭 입력/선택"
                  value={acc.character_name ?? ""}
                  list={`cuo-character-candidates-${acc.id}`}
                  onChange={(e) =>
                    updateAccount(acc.id, { character_name: e.target.value })
                  }
                  autoComplete="off"
                  title="CUO 프로필 기록이 있으면 후보로 표시됩니다. 직접 입력도 가능합니다."
                />
                <datalist id={`cuo-character-candidates-${acc.id}`}>
                  {characterCandidatesFor(acc.username).map((character) => (
                    <option key={character} value={character} />
                  ))}
                </datalist>
              </div>
              <button
                type="button"
                className="btn-action btn-action-danger"
                onClick={() => removeAccount(acc.id)}
                title="이 계정 제거"
              >
                삭제
              </button>
              <span className="account-index">#{i + 1}</span>
            </div>
          );
        })}
      </section>

      {draft.server.accounts.length > 0 && (
        <section className="form-section layout-editor-section">
          <header className="form-section-title layout-editor-header">
            <span>
              보조 모니터 창 배치
              <span className="form-hint">계정을 원하는 위치로 드래그</span>
            </span>
            <select
              className="text-input layout-preset-select"
              value={layoutPreset}
              onChange={(event) =>
                setLayoutPreset(event.target.value as SecondaryLayoutPreset)
              }
            >
              <option value="two_by_two">2×2 · 보조창 4개</option>
              <option value="two_by_two_center">2×2 + 중앙 · 보조창 5개</option>
            </select>
          </header>

          {selectedForMulti[0] && (
            <div className="layout-leader-row">
              <span className="layout-leader-badge">리더 · 자동 배치 제외</span>
              <span>{accountLabel(selectedForMulti[0])}</span>
            </div>
          )}

          <div
            className={`layout-preview ${
              layoutPreset === "two_by_two_center" ? "has-center" : ""
            }`}
          >
            {layoutSlots.map((slot) => {
              const occupant = secondaryAccounts.find(
                (account) => account.secondary_slot === slot
              );
              return (
                <div
                  key={slot}
                  className={`layout-slot layout-slot-${slot} ${
                    occupant ? "is-occupied" : ""
                  } ${dragOverTarget === slot ? "is-drop-target" : ""}`}
                  data-layout-drop={slot}
                >
                  <span className="layout-slot-label">{SLOT_LABELS[slot]}</span>
                  {occupant ? (
                    <button
                      type="button"
                      className={`layout-account-chip ${
                        draggedAccountId === occupant.id ? "is-dragging" : ""
                      }`}
                      onPointerDown={(event) =>
                        beginAccountPointerDrag(event, occupant.id, slot)
                      }
                      onPointerMove={updateAccountPointerDrag}
                      onPointerUp={finishAccountPointerDrag}
                      onPointerCancel={cancelAccountPointerDrag}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          moveAccountToSlot(occupant.id, null);
                        }
                      }}
                      title="다른 슬롯으로 드래그하거나 클릭하여 자동 배치에서 제외"
                    >
                      {accountLabel(occupant)}
                    </button>
                  ) : (
                    <span className="layout-slot-empty">여기에 놓기</span>
                  )}
                </div>
              );
            })}
          </div>

          <div
            className={`layout-unassigned ${
              dragOverTarget === "unassigned" ? "is-drop-target" : ""
            }`}
            data-layout-drop="unassigned"
          >
            <span className="layout-unassigned-label">자동 배치하지 않음</span>
            <div className="layout-unassigned-list">
              {unassignedSecondaryAccounts.length > 0 ? (
                unassignedSecondaryAccounts.map((account) => (
                  <button
                    type="button"
                    key={account.id}
                    className={`layout-account-chip ${
                      draggedAccountId === account.id ? "is-dragging" : ""
                    }`}
                    onPointerDown={(event) =>
                      beginAccountPointerDrag(event, account.id, null)
                    }
                    onPointerMove={updateAccountPointerDrag}
                    onPointerUp={finishAccountPointerDrag}
                    onPointerCancel={cancelAccountPointerDrag}
                    title="위 배치 슬롯으로 드래그"
                  >
                    {accountLabel(account)}
                  </button>
                ))
              ) : (
                <span className="layout-unassigned-empty">미배정 보조 계정 없음</span>
              )}
            </div>
          </div>
          <p className="layout-apply-note">
            저장 후 새로 실행하거나 개별 복구하는 보조 클라이언트부터 적용됩니다.
          </p>
        </section>
      )}
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

function uniqueSorted(values: string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const value of values) {
    const trimmed = value.trim();
    const key = trimmed.toLowerCase();
    if (!trimmed || seen.has(key)) continue;
    seen.add(key);
    result.push(trimmed);
  }
  return result.sort((a, b) => a.localeCompare(b));
}
