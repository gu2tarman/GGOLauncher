import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { PluginEntry } from "./types";

type Props = {
  plugins: PluginEntry[];
  onChange: (next: PluginEntry[]) => void;
};

function filenameOf(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

function stemOf(path: string): string {
  const fn = filenameOf(path);
  const dot = fn.lastIndexOf(".");
  return dot > 0 ? fn.slice(0, dot) : fn;
}

/** 알려진 플러그인 단축 표기 (대소문자 무시). 매칭 안되면 stem 그대로. */
const NICKNAMES: Record<string, string> = {
  razorenhanced: "라죠",
  razor: "라죠",
  classicassist: "클어시",
};

function autoNameOf(path: string): string {
  const stem = stemOf(path);
  const key = stem.toLowerCase();
  return NICKNAMES[key] ?? stem;
}

/** PluginEntry → 화면 표시명. display_name이 있으면 우선. */
function displayNameOfEntry(p: PluginEntry): string {
  const dn = p.display_name?.trim();
  if (dn) return dn;
  return autoNameOf(p.path);
}

function typeOf(path: string): "dll" | "exe" | "?" {
  const ext = path.toLowerCase().split(".").pop();
  if (ext === "dll") return "dll";
  if (ext === "exe") return "exe";
  return "?";
}

export function PluginPanel({ plugins, onChange }: Props) {
  /** 한 번에 하나만 활성. 플러그인이 1개 이상이면 반드시 하나는 선택된 상태. */
  const setSelected = (newList: typeof plugins, selectIdx: number) =>
    newList.map((p, i) => ({ ...p, enabled: selectIdx === i }));

  const activeIdx = plugins.findIndex((p) => p.enabled);

  // 인라인 이름 편집 상태
  const [editingIdx, setEditingIdx] = useState<number | null>(null);
  const [editDraft, setEditDraft] = useState<string>("");
  const editInputRef = useRef<HTMLInputElement | null>(null);
  const suppressBlurCommitRef = useRef(false);

  useEffect(() => {
    if (editingIdx !== null && editInputRef.current) {
      editInputRef.current.focus();
      editInputRef.current.select();
    }
  }, [editingIdx]);

  const startEdit = (i: number) => {
    suppressBlurCommitRef.current = false;
    setEditingIdx(i);
    setEditDraft(plugins[i].display_name ?? autoNameOf(plugins[i].path));
  };
  const commitEdit = () => {
    if (editingIdx === null) return;
    const trimmed = editDraft.trim();
    const next = plugins.map((p, i) =>
      i === editingIdx
        ? {
            ...p,
            // 빈 문자열이면 별명 제거 (자동 이름으로 폴백)
            display_name: trimmed.length > 0 ? trimmed : null,
          }
        : p
    );
    onChange(next);
    setEditingIdx(null);
  };
  const cancelEdit = () => {
    suppressBlurCommitRef.current = true;
    setEditingIdx(null);
  };

  // 불변식 보강: 플러그인이 있는데 아무도 활성 아니면 첫 번째 자동 선택
  useEffect(() => {
    if (plugins.length > 0 && activeIdx === -1) {
      onChange(setSelected(plugins, 0));
    }
  }, [plugins, activeIdx, onChange]);

  const onAdd = async () => {
    try {
      const picked = await api.pluginSelectFile();
      if (!picked) return;
      if (plugins.some((p) => p.path === picked)) {
        alert("이미 등록된 플러그인입니다.");
        return;
      }
      // 새로 추가한 항목을 활성으로
      const appended = [...plugins, { path: picked, enabled: false }];
      onChange(setSelected(appended, appended.length - 1));
    } catch (e) {
      alert(`추가 실패: ${e}`);
    }
  };

  const onImportZip = async () => {
    try {
      const dllPath = await api.importPluginFromZip();
      if (!dllPath) return;
      if (plugins.some((p) => p.path === dllPath)) {
        alert("이미 등록된 플러그인입니다.");
        return;
      }
      const appended = [...plugins, { path: dllPath, enabled: false }];
      onChange(setSelected(appended, appended.length - 1));
    } catch (e) {
      alert(`ZIP import 실패: ${e}`);
    }
  };

  const onSelect = (idx: number) => {
    if (idx === activeIdx) return; // 같은 거 재클릭은 무시 (해제 불가)
    onChange(setSelected(plugins, idx));
  };

  const onRemove = (idx: number) => {
    const target = plugins[idx];
    if (!target) return;
    if (
      !confirm(
        `"${displayNameOfEntry(target)}" 제거할까요?\n(파일 자체는 삭제되지 않습니다)`
      )
    )
      return;
    const remaining = plugins.filter((_, i) => i !== idx);
    // 활성을 지운 경우 남은 첫 항목 자동 선택. 다 비면 그냥 빈 배열.
    if (idx === activeIdx && remaining.length > 0) {
      onChange(setSelected(remaining, 0));
    } else {
      onChange(remaining);
    }
  };

  return (
    <section className="plugin-panel">
      <header className="panel-header panel-header-row">
        <span>
          플러그인
          {activeIdx >= 0 && (
            <span className="plugin-count">{displayNameOfEntry(plugins[activeIdx])}</span>
          )}
        </span>
        <div className="panel-actions">
          <button className="btn-small" onClick={onAdd}>
            + Add
          </button>
          <button className="btn-small" onClick={onImportZip}>
            Import ZIP
          </button>
        </div>
      </header>
      <div className="plugin-grid">
        {plugins.length === 0 ? (
          <div className="plugin-empty">등록된 플러그인이 없습니다</div>
        ) : (
          plugins.map((plugin, i) => {
            const isActive = i === activeIdx;
            return (
              <div
                key={`${plugin.path}_${i}`}
                className={`plugin-item ${isActive ? "is-selected" : ""}`}
                onClick={() => onSelect(i)}
                role="radio"
                aria-checked={isActive}
                tabIndex={0}
                title={
                  isActive
                    ? `현재 선택됨\n경로: ${plugin.path}`
                    : `클릭하여 선택\n경로: ${plugin.path}`
                }
              >
                <input
                  type="radio"
                  checked={isActive}
                  onChange={() => onSelect(i)}
                  onClick={(e) => e.stopPropagation()}
                  name="active-plugin"
                />
                <span className={`plugin-type plugin-type-${typeOf(plugin.path)}`}>
                  {typeOf(plugin.path).toUpperCase()}
                </span>
                {editingIdx === i ? (
                  <input
                    ref={editInputRef}
                    className="text-input"
                    style={{ flex: 1, fontSize: 12, padding: "2px 6px" }}
                    value={editDraft}
                    onChange={(e) => setEditDraft(e.target.value)}
                    onClick={(e) => e.stopPropagation()}
                    onBlur={() => {
                      if (suppressBlurCommitRef.current) {
                        suppressBlurCommitRef.current = false;
                        return;
                      }
                      commitEdit();
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        commitEdit();
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        cancelEdit();
                      }
                    }}
                    placeholder={autoNameOf(plugin.path)}
                  />
                ) : (
                  <span
                    className="plugin-name"
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      startEdit(i);
                    }}
                    title="더블클릭으로 이름 수정"
                    style={{ cursor: "text" }}
                  >
                    {displayNameOfEntry(plugin)}
                  </span>
                )}
                {editingIdx !== i && (
                  <button
                    className="btn-plugin-remove"
                    onClick={(e) => {
                      e.stopPropagation();
                      startEdit(i);
                    }}
                    title="이름 수정"
                    aria-label="이름 수정"
                    style={{ fontSize: 11 }}
                  >
                    ✎
                  </button>
                )}
                <button
                  className="btn-plugin-remove"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemove(i);
                  }}
                  title="제거"
                  aria-label="제거"
                >
                  ×
                </button>
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
