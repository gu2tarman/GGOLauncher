import { useMemo, useState } from "react";
import { api } from "./api";
import { Modal } from "./Modal";
import type { ImportPreview, ImportRequest, ImportResult, ImportScan, Settings } from "./types";

const kindNames = { cuo: "ClassicUO → GGO CE", re: "Razor Enhanced", ca: "ClassicAssist" };
const labels: Record<string, string> = {
  "profile.json": "화면·게임 설정", "macros.xml": "매크로·단축키", "gumps.xml": "창 배치",
  "RazorEnhanced.settings.GENERAL": "일반 설정 (프로필 필수)",
  "RazorEnhanced.settings.HOTKEYS": "핫키",
  "RazorEnhanced.settings.SCRIPTING": "스크립트 목록·단축키·자동 실행 설정",
  Macros: "매크로", Hotkeys: "핫키", General: "일반 설정", Options: "옵션",
};

export function ProfileImportModal({ settings, initialProfileId, onClose }: {
  settings: Settings; initialProfileId: string | null; onClose: () => void;
}) {
  const defaultCuo = settings.profiles.find(p => p.id === initialProfileId)?.cuo_path ?? settings.profiles[0]?.cuo_path ?? "";
  const [request, setRequest] = useState<ImportRequest>({ kind: "cuo", source: "", destination: defaultCuo, selected: [], replace: false });
  const [scan, setScan] = useState<ImportScan | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");
  const [closed, setClosed] = useState(false);

  const targets = useMemo(() => request.kind === "cuo"
    ? settings.profiles.filter(p => p.cuo_path).map(p => ({ label: p.name, path: p.cuo_path! }))
    : settings.plugins.filter(p => p.path.toLowerCase().endsWith(request.kind === "re" ? "razorenhanced.exe" : "classicassist.dll"))
      .map(p => ({ label: p.display_name || kindNames[request.kind], path: p.path.replace(/[\\/][^\\/]+$/, "") })), [settings, request.kind]);
  const groups = useMemo(() => {
    const map = new Map<string, ImportScan["items"]>();
    for (const item of scan?.items ?? []) {
      if (filter && !`${item.group} ${item.label} ${labels[item.label] ?? ""}`.toLowerCase().includes(filter.toLowerCase())) continue;
      map.set(item.group, [...(map.get(item.group) ?? []), item]);
    }
    return [...map];
  }, [scan, filter]);

  const reset = (patch: Partial<ImportRequest>) => {
    setRequest(r => ({ ...r, ...patch, selected: [] }));
    setScan(null); setPreview(null); setResult(null); setError(""); setFilter(""); setClosed(false);
  };
  const selectKind = (kind: ImportRequest["kind"]) => {
    const plugin = settings.plugins.find(p => p.path.toLowerCase().endsWith(kind === "re" ? "razorenhanced.exe" : "classicassist.dll"));
    reset({ kind, source: "", destination: kind === "cuo" ? defaultCuo : plugin?.path.replace(/[\\/][^\\/]+$/, "") ?? "" });
  };
  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError("");
    try { await action(); } catch(e) { setError(String(e)); } finally { setBusy(false); }
  };
  const pick = (field: "source" | "destination") => run(async () => {
    const path = await api.importSelectDirectory(); if (path) reset({ [field]: path });
  });
  const toggle = (ids: string[], checked: boolean) => {
    setRequest(r => ({ ...r, selected: checked ? [...new Set([...r.selected, ...ids])] : r.selected.filter(id => !ids.includes(id)) }));
    setPreview(null); setResult(null); setError("");
  };

  return <Modal open onClose={() => { if (!busy) onClose(); }} title="프로필 가져오기" width={900}>
    <div className="profile-import">
      <p className="import-intro">기존 설정을 골라 가져와 CE에서 이어서 사용하세요. 원본 설치 폴더는 유지됩니다.</p>
      <details className="import-source" open={!scan}>
      <summary>원본 및 대상 폴더 · {kindNames[request.kind]}</summary>
      <fieldset disabled={busy || !!result} className="import-fields">
        <label>가져올 프로그램
          <select className="text-input" value={request.kind} onChange={e => selectKind(e.target.value as ImportRequest["kind"])}>
            {Object.entries(kindNames).map(([value,label]) => <option value={value} key={value}>{label}</option>)}
          </select>
        </label>
        <label>원본 설치 폴더
          <div className="import-path"><input className="text-input" value={request.source} placeholder="기존 프로그램이 설치된 폴더" onChange={e => reset({ source: e.target.value })}/><button className="btn-action" onClick={() => pick("source")}>찾아보기</button></div>
        </label>
        <label>가져올 대상
          <select className="text-input" value={targets.some(t => t.path === request.destination) ? request.destination : ""} onChange={e => { if(e.target.value) reset({ destination: e.target.value }); }}>
            <option value="">대상 폴더 직접 선택</option>
            {targets.map((t,i) => <option key={i} value={t.path}>{t.label} · {t.path}</option>)}
          </select>
          <div className="import-path"><input className="text-input" value={request.destination} placeholder="CE 또는 보조 프로그램 설치 폴더" onChange={e => reset({ destination: e.target.value })}/><button className="btn-action" onClick={() => pick("destination")}>찾아보기</button></div>
        </label>
        {request.kind !== "cuo" && <p className="import-hint">보조 프로그램 설정은 같은 설치 경로를 사용하는 모든 런처 프로필에서 공유됩니다.</p>}
        <button className="btn-primary" disabled={!request.source.trim() || !request.destination.trim()} onClick={() => run(async () => {
          setScan(null); setPreview(null); setRequest(r => ({ ...r, selected: [] }));
          setScan(await api.profileImportScan(request));
        })}>가져올 항목 검색</button>
      </fieldset>
      </details>

      {scan && !result && <fieldset className="import-fields" disabled={busy}>
        <div className="import-toolbar">
          <strong>항목 선택 · {request.selected.length}개</strong>
          <input className="text-input" aria-label="가져올 항목 검색어" placeholder="캐릭터·프로필·파일 검색" value={filter} onChange={e => setFilter(e.target.value)}/>
          <button className="btn-action" onClick={() => toggle(groups.flatMap(([,items]) => items.map(i => i.id)), true)}>검색 결과 선택</button>
          <button className="btn-action" onClick={() => toggle(request.selected, false)}>선택 해제</button>
        </div>
        {!scan.items.length && <p>지원하는 프로필을 찾지 못했습니다. 원본 설치 폴더와 저장 형식을 확인해주세요.</p>}
        <div className="import-items">
          {groups.map(([group,items]) => <details key={group} open={groups.length <= 3 || !!filter}>
            <summary>{group} <span>{items.filter(i => request.selected.includes(i.id)).length} / {items.length}</span></summary>
            <label className="import-check"><input type="checkbox" checked={items.every(i => request.selected.includes(i.id))} onChange={e => toggle(items.map(i => i.id), e.target.checked)}/> 이 그룹 선택</label>
            {items.map(item => <label className="import-check" key={item.id}>
              <input type="checkbox" checked={request.selected.includes(item.id)} onChange={e => toggle([item.id], e.target.checked)}/>
              <span>{labels[item.label] ?? item.label}{labels[item.label] && <small>{item.label}</small>}</span>
            </label>)}
          </details>)}
        </div>
        <label className="import-check"><input type="checkbox" checked={request.replace} onChange={e => { setRequest(r => ({ ...r, replace: e.target.checked })); setPreview(null); }}/>
          같은 이름의 기존 항목 교체 (자동 백업) — CA는 선택한 설정 항목만 교체</label>
        <button className="btn-primary" disabled={!request.selected.length} onClick={() => run(async () => { setPreview(null); setClosed(false); setPreview(await api.profileImportPreview(request)); })}>가져오기 미리보기</button>
      </fieldset>}

      {preview && !result && <div className="import-preview">
        <strong>반영될 파일 · {preview.changes.length}개</strong>
        <div className="import-change-list">{preview.changes.map(change => <div className="import-change" key={change.path}><b>{change.action}</b><span>{change.path}</span></div>)}</div>
        <ul className="import-notes">{preview.notes.map((note,i) => <li key={i}>{note}</li>)}</ul>
        <label className="import-check"><input type="checkbox" disabled={busy} checked={closed} onChange={e => setClosed(e.target.checked)}/> 게임과 RE·CA를 종료했고, 대상 폴더와 반영 항목을 확인했습니다.</label>
        <button className="btn-primary" disabled={busy || !closed || !preview.changes.some(c => c.action === "새로 복사" || c.action === "백업 후 반영")} onClick={() => run(async () => setResult(await api.profileImportApply(request, preview.fingerprint)))}>선택 항목 가져오기</button>
      </div>}
      {scan && !preview && !result && <ul className="import-notes">{scan.notes.map((note,i) => <li key={i}>{note}</li>)}</ul>}
      {busy && <p role="status">처리 중입니다…</p>}
      {error && <div className="import-error" role="alert">{error}</div>}
      {result && <div className="import-result" role="status">
        <strong>가져오기 완료 · {result.copied}개 파일 반영 / {result.skipped}개 유지</strong>
        <p>복사된 파일의 내용을 검증했습니다.</p>
        <p>백업 및 복구 목록: <span className="import-backup-path">{result.backup}</span></p>
        <p>{request.kind === "cuo" ? "같은 계정과 캐릭터로 CE에 접속해 설정을 확인해주세요." : "보조 프로그램을 시작한 뒤 가져온 프로필을 선택하고 핫키·스크립트 연결을 확인해주세요."}</p>
        <button className="btn-action" onClick={() => reset({ selected: [] })}>다른 항목 가져오기</button>
      </div>}
    </div>
  </Modal>;
}
