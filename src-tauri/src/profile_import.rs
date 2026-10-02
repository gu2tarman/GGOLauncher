//! Selection-based data migration. Never copies application binaries or login
//! settings. Preview and execution use the same plan, checked by content hashes.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static IMPORT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Cuo,
    Re,
    Ca,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub kind: Kind,
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub selected: Vec<String>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub group: String,
    pub label: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub items: Vec<Item>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub path: String,
    pub action: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub fingerprint: String,
    pub changes: Vec<Change>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub copied: usize,
    pub skipped: usize,
    pub backup: String,
    pub notes: Vec<String>,
}

struct Roots {
    install: PathBuf,
    profiles: PathBuf,
    resources: PathBuf,
}
struct Candidate {
    item: Item,
    source: PathBuf,
    dest: PathBuf,
    section: Option<String>,
    re_profile: Option<String>,
}
struct Write {
    path: PathBuf,
    bytes: Vec<u8>,
    before: Option<Vec<u8>>,
}
struct Plan {
    preview: Preview,
    writes: Vec<Write>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
        .map_err(|e| format!("JSON 형식 확인 필요: {}: {e}", path.display()))
}
fn optional_json(path: &Path) -> Result<Value, String> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(json!({}))
    }
}

// Check every component, including junctions on Windows, before following paths.
fn no_links(path: &Path) -> Result<(), String> {
    for p in path.ancestors() {
        match fs::symlink_metadata(p) {
            Ok(m) => {
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    m.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let linked = m.file_type().is_symlink();
                if linked {
                    return Err(format!(
                        "연결된 폴더/파일은 직접 경로로 선택해주세요: {}",
                        p.display()
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(err(e)),
        }
    }
    Ok(())
}

fn absolute(base: &Path, configured: Option<&str>, fallback: &str) -> Result<PathBuf, String> {
    let value = configured
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback);
    let p = Path::new(value);
    let p = if p.is_absolute() {
        p.to_owned()
    } else {
        base.join(p)
    };
    // Resolve existing ancestors and normalize '..' even for new destinations.
    no_links(&p)?;
    let mut missing = Vec::new();
    let mut ancestor = p.as_path();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("경로를 확인해주세요")?
                .to_owned(),
        );
        ancestor = ancestor.parent().ok_or("경로를 확인해주세요")?;
    }
    let mut out = fs::canonicalize(ancestor).map_err(err)?;
    for part in missing.into_iter().rev() {
        out.push(part);
    }
    Ok(out)
}

fn roots(path: &str, kind: Kind) -> Result<Roots, String> {
    let path = Path::new(path.trim());
    if !path.is_absolute() || !path.is_dir() {
        return Err("설치 폴더의 전체 경로를 선택해주세요.".into());
    }
    no_links(path)?;
    let install = fs::canonicalize(path).map_err(err)?;
    let executable = match kind {
        Kind::Cuo => "ClassicUO.exe",
        Kind::Re => "RazorEnhanced.exe",
        Kind::Ca => "ClassicAssist.dll",
    };
    if !install.join(executable).is_file() {
        return Err(format!("{executable}가 있는 설치 폴더를 선택해주세요."));
    }
    let config = optional_json(&install.join(match kind {
        Kind::Cuo => "settings.json",
        Kind::Ca => "Assistant.json",
        Kind::Re => "__unused_import_config__",
    }))?;
    let profiles = match kind {
        Kind::Cuo => absolute(&install, config["profilespath"].as_str(), "Data/Profiles")?,
        Kind::Ca => absolute(&install, config["ProfileDirectory"].as_str(), "Profiles")?,
        Kind::Re => install.join("Profiles"),
    };
    let resources = if kind == Kind::Ca {
        absolute(&install, config["GlobalDirectory"].as_str(), ".")?
    } else {
        install.clone()
    };
    Ok(Roots {
        install,
        profiles,
        resources,
    })
}

fn overlaps(a: &Path, b: &Path) -> bool {
    // Windows file systems are normally case insensitive; canonicalize doesn't
    // guarantee matching case for every existing ancestor.
    let key = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    let a = key(a);
    let b = key(b);
    a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\"))
}

fn paired(request: &Request) -> Result<(Roots, Roots), String> {
    let src = roots(&request.source, request.kind)?;
    let dst = roots(&request.destination, request.kind)?;
    for a in [&src.install, &src.profiles, &src.resources] {
        for b in [&dst.install, &dst.profiles, &dst.resources] {
            if overlaps(a, b) {
                return Err(
                    "원본과 대상이 같거나 서로 포함된 폴더입니다. 별도 설치 폴더를 선택해주세요."
                        .into(),
                );
            }
        }
    }
    Ok((src, dst))
}

fn ignored(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    name.starts_with('.')
        || matches!(
            name.as_str(),
            "backup" | "backups" | "conflict" | "__pycache__" | "node_modules"
        )
        || matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase()
                .as_str(),
            "bak" | "lock" | "log" | "tmp" | "pyc" | "pdb" | "exe" | "dll"
        )
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    no_links(dir)?;
    for entry in fs::read_dir(dir).map_err(err)? {
        let p = entry.map_err(err)?.path();
        if ignored(&p) {
            continue;
        }
        no_links(&p)?;
        if p.is_dir() {
            walk(&p, files)?;
        } else if p.is_file() {
            files.push(p);
        }
    }
    Ok(())
}

fn inventory(r: &Request, src: &Roots, dst: &Roots) -> Result<Vec<Candidate>, String> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    walk(&src.profiles, &mut files)?;
    for file in files {
        let rel = file.strip_prefix(&src.profiles).map_err(err)?;
        let name = file.file_name().unwrap().to_string_lossy();
        let depth = rel.components().count();
        let re_profile = if r.kind == Kind::Re
            && depth == 2
            && name.starts_with("RazorEnhanced.settings.")
            && !name.ends_with("PASSWORD")
        {
            Some(
                rel.components()
                    .next()
                    .unwrap()
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
        let allowed = match r.kind {
            Kind::Cuo => {
                (depth == 4 || depth == 1)
                    && matches!(
                        file.extension().and_then(|s| s.to_str()),
                        Some("json" | "xml")
                    )
                    && name != "lastcharacter.json"
            }
            Kind::Re => re_profile.is_some(),
            Kind::Ca => depth == 1 && file.extension().and_then(|s| s.to_str()) == Some("json"),
        };
        if !allowed {
            continue;
        }
        let group = match r.kind {
            Kind::Re => format!("RE / {}", re_profile.as_deref().unwrap()),
            Kind::Ca => format!("CA / {name}"),
            Kind::Cuo => format!(
                "CUO / {}",
                rel.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or("공통 설정".into())
            ),
        };
        let bytes = fs::metadata(&file).map_err(err)?.len();
        let base_id = format!("profiles/{}", rel.to_string_lossy().replace('\\', "/"));
        let sections = if r.kind == Kind::Ca {
            let value = read_json(&file)?;
            let obj = value
                .as_object()
                .ok_or("CA 프로필은 JSON 객체여야 합니다.")?;
            obj.keys()
                .filter(|k| !matches!(k.as_str(), "Name" | "Hash"))
                .map(|k| Some(k.clone()))
                .collect::<Vec<_>>()
        } else {
            vec![None]
        };
        for section in sections {
            let id = section
                .as_ref()
                .map(|key| format!("{base_id}::{key}"))
                .unwrap_or(base_id.clone());
            out.push(Candidate {
                item: Item {
                    id,
                    group: group.clone(),
                    label: section.clone().unwrap_or(name.to_string()),
                    bytes,
                },
                source: file.clone(),
                dest: dst.profiles.join(rel),
                section,
                re_profile: re_profile.clone(),
            });
        }
    }
    if r.kind != Kind::Cuo {
        let folders: &[&str] = if r.kind == Kind::Re {
            &["Scripts", "Macros"]
        } else {
            &["Macros", "Modules"]
        };
        for folder in folders {
            let base = src.resources.join(folder);
            let mut files = Vec::new();
            walk(&base, &mut files)?;
            for file in files {
                let rel = file.strip_prefix(&base).map_err(err)?;
                let id = format!("{folder}/{}", rel.to_string_lossy().replace('\\', "/"));
                out.push(Candidate {
                    item: Item {
                        id,
                        group: folder.to_string(),
                        label: rel.to_string_lossy().into_owned(),
                        bytes: fs::metadata(&file).map_err(err)?.len(),
                    },
                    source: file.clone(),
                    dest: dst.resources.join(folder).join(rel),
                    section: None,
                    re_profile: None,
                });
            }
        }
        // RE and recent CA profiles can reference scripts outside the install
        // folder. Offer those exact files individually; never sweep a user's
        // whole Documents/workspace tree implicitly.
        let profile_files: BTreeSet<PathBuf> = out
            .iter()
            .filter(|c| {
                c.section.is_some()
                    || c.source.extension().and_then(|s| s.to_str()) == Some("SCRIPTING")
            })
            .map(|c| c.source.clone())
            .collect();
        let mut references = BTreeSet::new();
        for path in profile_files {
            script_references(&read_json(&path)?, &mut references);
        }
        let mut known: BTreeSet<PathBuf> = out.iter().map(|c| c.source.clone()).collect();
        for reference in references {
            let path = Path::new(&reference);
            if !path.is_absolute() || !path.is_file() || ignored(path) {
                continue;
            }
            no_links(path)?;
            let path = fs::canonicalize(path).map_err(err)?;
            if !known.insert(path.clone())
                || overlaps(&path, &dst.install)
                || overlaps(&path, &dst.resources)
            {
                continue;
            }
            let folder_hash = hash(path.parent().unwrap().to_string_lossy().as_bytes());
            let folder = if r.kind == Kind::Re {
                "Scripts"
            } else {
                "Macros"
            };
            let rel = PathBuf::from(folder)
                .join("Imported")
                .join(&folder_hash[..16])
                .join(path.file_name().unwrap());
            out.push(Candidate {
                item: Item {
                    id: format!("external/{}", rel.to_string_lossy().replace('\\', "/")),
                    group: "외부 참조 스크립트".into(),
                    label: display_path(&path),
                    bytes: fs::metadata(&path).map_err(err)?.len(),
                },
                source: path,
                dest: dst.resources.join(rel),
                section: None,
                re_profile: None,
            });
        }
    }
    out.sort_by(|a, b| a.item.id.cmp(&b.item.id));
    Ok(out)
}

fn script_references(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(obj) => {
            for (key, v) in obj {
                if matches!(key.as_str(), "FullPath" | "FilePath") {
                    if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
                        out.insert(s.to_owned());
                    }
                } else {
                    script_references(v, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                script_references(item, out);
            }
        }
        _ => (),
    }
}

fn notes(kind: Kind) -> Vec<String> {
    let mut out = vec!["원본은 유지됩니다. 같은 이름의 대상 파일은 기본적으로 건너뛰며, 교체를 선택하면 먼저 백업합니다.".into()];
    match kind {
        Kind::Cuo => out.push("계정/서버/캐릭터 폴더 이름을 그대로 유지합니다. 런처의 접속 계정과 서버가 같아야 적용됩니다. 로그인 정보와 CE 버전 설정은 복사하지 않습니다.".into()),
        Kind::Re => out.push("새 RE 프로필에는 필수 GENERAL 설정과 프로필 등록을 함께 가져옵니다. 핫키와 스크립트 목록은 별도 선택 가능합니다. 스크립트 파일도 함께 선택해주세요.".into()),
        Kind::Ca => out.push("CA 프로필의 항목별로 선택합니다. Macros와 Hotkeys를 함께 선택하면 연결을 유지하기 쉽습니다. 가져온 프로필은 CA에서 선택해주세요. 사용자 지정 ProfileDirectory/GlobalDirectory도 반영합니다.".into()),
    }
    if kind != Kind::Cuo {
        out.push("외부 참조 스크립트도 선택해 복사할 수 있습니다. 스크립트 내부의 고정 경로와 별도 모듈·DLL 의존성은 직접 확인해주세요. 선택하지 않았거나 없는 참조 파일은 미리보기에 안내합니다.".into());
    }
    out
}

pub fn scan(r: &Request) -> Result<Scan, String> {
    let (src, dst) = paired(r)?;
    Ok(Scan {
        items: inventory(r, &src, &dst)?
            .into_iter()
            .map(|x| x.item)
            .collect(),
        notes: notes(r.kind),
    })
}

fn read_before(path: &Path) -> Result<Option<Vec<u8>>, String> {
    no_links(path)?;
    if path.exists() {
        Ok(Some(fs::read(path).map_err(err)?))
    } else {
        Ok(None)
    }
}

// Rebase only script references whose destination file will exist after this
// plan. References to external/unselected files stay intact and are reported.
fn rebase(value: &mut Value, mappings: &[(PathBuf, PathBuf)], notes: &mut BTreeSet<String>) {
    match value {
        Value::Object(obj) => {
            for (key, v) in obj {
                if matches!(key.as_str(), "FullPath" | "FilePath") {
                    if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
                        let normalized = s.replace('/', "\\").to_lowercase();
                        if let Some((_, dst)) = mappings.iter().find(|(src, _)| {
                            src.to_string_lossy()
                                .trim_start_matches("\\\\?\\")
                                .replace('/', "\\")
                                .to_lowercase()
                                == normalized.trim_start_matches("\\\\?\\")
                        }) {
                            *v = Value::String(display_path(dst));
                        } else {
                            notes.insert(format!("스크립트 경로 유지 (별도 파일 확인 필요): {s}"));
                        }
                    }
                } else {
                    rebase(v, mappings, notes);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                rebase(item, mappings, notes);
            }
        }
        _ => (),
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .to_string()
}
fn json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(err)
}

fn plan(r: &Request) -> Result<Plan, String> {
    let (src, dst) = paired(r)?;
    let all = inventory(r, &src, &dst)?;
    let selected: BTreeSet<&str> = r.selected.iter().map(String::as_str).collect();
    if selected.is_empty() {
        return Err("가져올 항목을 선택해주세요.".into());
    }
    if selected
        .iter()
        .any(|id| !all.iter().any(|c| c.item.id == *id))
    {
        return Err("원본 항목이 변경되었습니다. 다시 검색해주세요.".into());
    }
    let chosen: Vec<&Candidate> = all
        .iter()
        .filter(|c| selected.contains(c.item.id.as_str()))
        .collect();
    let mappings: Vec<_> = chosen
        .iter()
        .filter(|c| !c.item.id.starts_with("profiles/"))
        .map(|c| (c.source.clone(), c.dest.clone()))
        .collect();
    let mut extra_notes = BTreeSet::new();
    let mut content = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut source_hashes = BTreeMap::<PathBuf, String>::new();
    let mut ca_sections = BTreeMap::<PathBuf, Vec<&Candidate>>::new();
    let mut re_names = BTreeSet::new();
    for c in &chosen {
        no_links(&c.source)?;
        no_links(&c.dest)?;
        let bytes = fs::read(&c.source).map_err(err)?;
        if c.re_profile.is_some() {
            read_json(&c.source)?;
        }
        source_hashes.insert(c.source.clone(), hash(&bytes));
        if let Some(name) = &c.re_profile {
            re_names.insert(name.clone());
        }
        if c.section.is_some() {
            ca_sections.entry(c.dest.clone()).or_default().push(c);
            continue;
        }
        let bytes = if r.kind == Kind::Re
            && c.source.extension().and_then(|s| s.to_str()) == Some("SCRIPTING")
        {
            let mut v = read_json(&c.source)?;
            rebase(&mut v, &mappings, &mut extra_notes);
            json_bytes(&v)?
        } else {
            bytes
        };
        content.insert(c.dest.clone(), bytes);
    }
    for (path, sections) in ca_sections {
        let mut value = optional_json(&path)?;
        let obj = value
            .as_object_mut()
            .ok_or("대상 CA 프로필 형식을 확인해주세요.")?;
        let source = read_json(&sections[0].source)?;
        let mut changed = false;
        for c in sections {
            let key = c.section.as_ref().unwrap();
            // Conflict is per section for CA: keep unselected destination keys.
            if r.replace || !obj.contains_key(key) {
                let mut incoming = source[key].clone();
                rebase(&mut incoming, &mappings, &mut extra_notes);
                if obj.get(key) != Some(&incoming) {
                    changed = true;
                }
                obj.insert(key.clone(), incoming);
            }
        }
        if !changed && path.exists() {
            content.insert(path.clone(), fs::read(&path).map_err(err)?);
            continue;
        }
        value["Name"] = json!(path.file_name().unwrap().to_string_lossy());
        value.as_object_mut().unwrap().remove("Hash");
        content.insert(path, json_bytes(&value)?);
    }
    if r.kind == Kind::Re {
        for name in &re_names {
            let general = dst
                .profiles
                .join(name)
                .join("RazorEnhanced.settings.GENERAL");
            if !general.exists() && !content.contains_key(&general) {
                let source = src
                    .profiles
                    .join(name)
                    .join("RazorEnhanced.settings.GENERAL");
                no_links(&source)?;
                let bytes = fs::read(&source)
                    .map_err(|e| format!("{name}: 필수 GENERAL 설정을 읽지 못했습니다: {e}"))?;
                read_json(&source)?;
                source_hashes.insert(source, hash(&bytes));
                content.insert(general, bytes);
                extra_notes.insert(format!(
                    "{name}: 새 프로필의 필수 GENERAL 설정을 함께 가져옵니다."
                ));
            }
        }
        let index_path = dst.profiles.join("RazorEnhanced.NewProfiles");
        let source_path = src.profiles.join("RazorEnhanced.NewProfiles");
        no_links(&source_path)?;
        no_links(&index_path)?;
        let source = if source_path.exists() {
            let b = fs::read(&source_path).map_err(err)?;
            source_hashes.insert(source_path.clone(), hash(&b));
            read_json(&source_path)?
        } else {
            json!([])
        };
        if !source.is_array() {
            return Err("원본 RE 프로필 등록 목록 형식을 확인해주세요.".into());
        }
        let mut target = if index_path.exists() {
            read_json(&index_path)?
        } else {
            json!([])
        };
        let target = target
            .as_array_mut()
            .ok_or("대상 RE 프로필 등록 목록 형식을 확인해주세요.")?;
        let mut added = false;
        if !index_path.exists() && dst.profiles.exists() {
            for entry in fs::read_dir(&dst.profiles).map_err(err)? {
                let p = entry.map_err(err)?.path();
                no_links(&p)?;
                if p.is_dir() && p.join("RazorEnhanced.settings.GENERAL").is_file() {
                    target.push(json!({"Name":p.file_name().unwrap().to_string_lossy(),"Last":target.is_empty(),"Players":[]}));
                    added = true;
                }
            }
        }
        for name in &re_names {
            if target.iter().any(|v| {
                v["Name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            }) {
                continue;
            }
            let mut incoming = source
                .as_array()
                .and_then(|a| {
                    a.iter().find(|v| {
                        v["Name"]
                            .as_str()
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    })
                })
                .cloned()
                .unwrap_or(json!({"Name":name,"Players":[]}));
            incoming["Name"] = json!(name);
            incoming["Last"] = json!(target.is_empty());
            if let Some(players) = incoming["Players"].as_array_mut() {
                players.retain(|p| {
                    !target
                        .iter()
                        .filter_map(|t| t["Players"].as_array())
                        .flatten()
                        .any(|existing| {
                            (p["PlayerSerial"].as_i64().unwrap_or(0) != 0
                                && existing["PlayerSerial"] == p["PlayerSerial"])
                                || (p["PlayerName"].as_str().is_some()
                                    && existing["PlayerName"] == p["PlayerName"])
                        })
                });
            }
            target.push(incoming);
            added = true;
        }
        if added {
            content.insert(index_path, json_bytes(&json!(target))?);
        }
    }
    let mut writes = Vec::new();
    let mut changes = Vec::new();
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(r).map_err(err)?);
    for (p, h) in source_hashes {
        digest.update(p.to_string_lossy().as_bytes());
        digest.update(h.as_bytes());
    }
    for (path, bytes) in content {
        let before = read_before(&path)?;
        digest.update(path.to_string_lossy().as_bytes());
        digest.update(hash(&bytes).as_bytes());
        digest.update(
            before
                .as_deref()
                .map(hash)
                .unwrap_or("missing".into())
                .as_bytes(),
        );
        let merge = (r.kind == Kind::Ca && path.starts_with(&dst.profiles))
            || path
                .file_name()
                .is_some_and(|n| n == "RazorEnhanced.NewProfiles");
        let action = if before.as_ref() == Some(&bytes) {
            "동일 · 건너뜀"
        } else if before.is_some() && !r.replace && !merge {
            "기존 유지"
        } else if before.is_some() {
            "백업 후 반영"
        } else {
            "새로 복사"
        };
        changes.push(Change {
            path: display_path(&path),
            action: action.into(),
            bytes: bytes.len(),
        });
        if action == "백업 후 반영" || action == "새로 복사" {
            writes.push(Write {
                path,
                bytes,
                before,
            });
        }
    }
    let mut notes = notes(r.kind);
    notes.extend(extra_notes);
    Ok(Plan {
        preview: Preview {
            fingerprint: format!("{:x}", digest.finalize()),
            changes,
            notes,
        },
        writes,
    })
}

pub fn preview(r: &Request) -> Result<Preview, String> {
    Ok(plan(r)?.preview)
}

pub fn apply(r: &Request, fingerprint: &str) -> Result<ImportResult, String> {
    let _guard = IMPORT_LOCK
        .try_lock()
        .map_err(|_| "다른 가져오기가 진행 중입니다.")?;
    ensure_apps_closed()?;
    let plan = plan(r)?;
    if plan.preview.fingerprint != fingerprint {
        return Err(
            "미리보기 이후 원본 또는 대상이 변경되었습니다. 미리보기를 다시 확인해주세요.".into(),
        );
    }
    let root = dirs::data_dir()
        .ok_or("백업 폴더를 찾지 못했습니다.")?
        .join("GGOLauncher")
        .join("import-backups");
    execute(plan, &root)
}

fn execute(plan: Plan, backup_root: &Path) -> Result<ImportResult, String> {
    execute_with_hook(plan, backup_root, |_| Ok(()))
}

fn execute_with_hook(
    plan: Plan,
    backup_root: &Path,
    before_write: impl Fn(usize) -> Result<(), String>,
) -> Result<ImportResult, String> {
    no_links(backup_root)?;
    fs::create_dir_all(backup_root).map_err(err)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_nanos();
    let backup = backup_root.join(format!("import-{stamp}"));
    fs::create_dir(&backup).map_err(err)?;
    // Back up every overwritten file and persist the recovery map before the
    // first destination write. Failures restore all touched files.
    let mut manifest = Vec::new();
    for (i, w) in plan.writes.iter().enumerate() {
        no_links(&w.path)?;
        if read_before(&w.path)? != w.before {
            return Err("대상 파일이 변경되었습니다. 미리보기를 다시 확인해주세요.".into());
        }
        if let Some(b) = &w.before {
            fs::write(backup.join(format!("{i}.bak")), b).map_err(err)?;
        }
        fs::write(backup.join(format!("{i}.new")), &w.bytes).map_err(err)?;
        manifest.push(json!({"target":display_path(&w.path),"backup":w.before.as_ref().map(|_|format!("{i}.bak")),"before_sha256":w.before.as_deref().map(hash),"after_sha256":hash(&w.bytes)}));
    }
    fs::write(backup.join("manifest.json"), json_bytes(&json!(manifest))?).map_err(err)?;
    let mut touched = Vec::<usize>::new();
    let result = (|| -> Result<(), String> {
        for (i, w) in plan.writes.iter().enumerate() {
            before_write(i)?;
            no_links(&w.path)?;
            if read_before(&w.path)? != w.before {
                return Err(format!("가져오기 중 대상이 변경됨: {}", w.path.display()));
            }
            fs::create_dir_all(w.path.parent().ok_or("대상 경로 오류")?).map_err(err)?;
            touched.push(i);
            fs::copy(backup.join(format!("{i}.new")), &w.path).map_err(err)?;
            if fs::read(&w.path).map_err(err)? != w.bytes {
                return Err("복사 후 내용 검증 실패".into());
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for i in touched.into_iter().rev() {
            let w = &plan.writes[i];
            let restore = if let Some(b) = &w.before {
                fs::write(&w.path, b)
            } else {
                fs::remove_file(&w.path)
            };
            if let Err(e) = restore {
                failures.push(format!("{}: {e}", w.path.display()));
            }
        }
        return Err(format!(
            "가져오기 실패: {error}\n복구 결과: {}\n백업: {}",
            if failures.is_empty() {
                "변경 파일 복구 완료".into()
            } else {
                failures.join("; ")
            },
            display_path(&backup)
        ));
    }
    fs::write(
        backup.join("completed.txt"),
        b"All destination contents verified.\n",
    )
    .map_err(err)?;
    Ok(ImportResult {
        copied: plan.writes.len(),
        skipped: plan.preview.changes.len() - plan.writes.len(),
        backup: display_path(&backup),
        notes: plan.preview.notes,
    })
}

#[cfg(windows)]
fn ensure_apps_closed() -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("실행 중인 프로그램을 확인하지 못했습니다.".into());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = Vec::new();
        let mut valid = Process32FirstW(snapshot, &mut entry);
        while valid != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            if matches!(
                name.to_lowercase().as_str(),
                "classicuo.exe"
                    | "razorenhanced.exe"
                    | "classicassist.exe"
                    | "classicassist.launcher.exe"
            ) {
                found.push(name);
            }
            valid = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        if !found.is_empty() {
            return Err(format!(
                "설정 덮어쓰기를 방지하려면 게임과 보조 프로그램을 종료해주세요: {}",
                found.join(", ")
            ));
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn ensure_apps_closed() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        src: PathBuf,
        dst: PathBuf,
    }
    impl Fixture {
        fn new(kind: Kind) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("ggo-import-test-{}-{stamp}", std::process::id()));
            let src = root.join("원본");
            let dst = root.join("대상");
            fs::create_dir_all(&src).unwrap();
            fs::create_dir_all(&dst).unwrap();
            let executable = match kind {
                Kind::Cuo => "ClassicUO.exe",
                Kind::Re => "RazorEnhanced.exe",
                Kind::Ca => "ClassicAssist.dll",
            };
            fs::write(src.join(executable), b"source binary").unwrap();
            fs::write(dst.join(executable), b"target binary").unwrap();
            Self { root, src, dst }
        }
        fn request(&self, kind: Kind, ids: &[&str], replace: bool) -> Request {
            Request {
                kind,
                source: display_path(&self.src),
                destination: display_path(&self.dst),
                selected: ids.iter().map(|s| s.to_string()).collect(),
                replace,
            }
        }
        fn source(&self, path: &str, bytes: &str) {
            write(&self.src.join(path), bytes);
        }
        fn dest(&self, path: &str, bytes: &str) {
            write(&self.dst.join(path), bytes);
        }
        fn run(&self, r: &Request) -> ImportResult {
            execute(plan(r).unwrap(), &self.root.join("backups")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.root.parent() == Some(std::env::temp_dir().as_path())
                && self
                    .root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ggo-import-test-")
            {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }
    fn write(path: &Path, bytes: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn cuo_selection_conflicts_backups_and_source_preservation() {
        let f = Fixture::new(Kind::Cuo);
        let macros = "Data/Profiles/account/server/캐릭터/macros.xml";
        f.source(macros, "new macros");
        f.dest(macros, "old macros");
        f.source("Data/Profiles/account/server/캐릭터/profile.json", "{}");
        f.source(
            "settings.json",
            r#"{"clientversion":"7.0.1.0","password":"never copy"}"#,
        );
        f.source(
            "Data/Profiles/account/server/캐릭터/profile.json.bak",
            "backup",
        );
        let mut r = f.request(
            Kind::Cuo,
            &["profiles/account/server/캐릭터/macros.xml"],
            false,
        );
        let scan = scan(&r).unwrap();
        assert_eq!(scan.items.len(), 2);
        assert_eq!(f.run(&r).copied, 0);
        r.replace = true;
        let result = f.run(&r);
        assert_eq!(result.copied, 1);
        assert_eq!(
            fs::read_to_string(f.dst.join(macros)).unwrap(),
            "new macros"
        );
        assert_eq!(
            fs::read_to_string(Path::new(&result.backup).join("0.bak")).unwrap(),
            "old macros"
        );
        assert_eq!(
            fs::read_to_string(f.src.join(macros)).unwrap(),
            "new macros"
        );
        assert!(!f.dst.join("settings.json").exists());
        assert!(!f
            .dst
            .join("Data/Profiles/account/server/캐릭터/profile.json")
            .exists());
        assert_eq!(
            fs::read(f.dst.join("ClassicUO.exe")).unwrap(),
            b"target binary"
        );
    }

    #[test]
    fn re_selected_components_register_new_profile_and_rebase_script_reference() {
        let f = Fixture::new(Kind::Re);
        f.source(
            "Profiles/새프로필/RazorEnhanced.settings.GENERAL",
            "[{\"SettingVersion\":1}]",
        );
        f.source("Profiles/새프로필/RazorEnhanced.settings.HOTKEYS", "[123]");
        f.source(
            "Profiles/새프로필/RazorEnhanced.settings.AUTOLOOT_ITEMS",
            "[999]",
        );
        f.source("Scripts/sub/main.py", "print('hello')");
        let scripts = json!([{"FullPath":display_path(&f.src.join("Scripts/sub/main.py")),"Filename":"main.py","Hotkey":123},{"FullPath":"D:\\external\\keep.py","Filename":"keep.py"}]);
        f.source(
            "Profiles/새프로필/RazorEnhanced.settings.SCRIPTING",
            &scripts.to_string(),
        );
        f.source("Profiles/RazorEnhanced.NewProfiles",r#"[{"Name":"새프로필","Last":true,"Players":[{"PlayerName":"new","PlayerSerial":42}]}]"#);
        f.dest(
            "Profiles/RazorEnhanced.NewProfiles",
            r#"[{"Name":"existing","Last":true,"Players":[]}]"#,
        );
        let r = f.request(
            Kind::Re,
            &[
                "profiles/새프로필/RazorEnhanced.settings.HOTKEYS",
                "profiles/새프로필/RazorEnhanced.settings.SCRIPTING",
                "Scripts/sub/main.py",
            ],
            false,
        );
        let result = f.run(&r);
        assert_eq!(result.copied, 5);
        assert!(result.notes.iter().any(|n| n.contains("external")));
        assert!(f
            .dst
            .join("Profiles/새프로필/RazorEnhanced.settings.GENERAL")
            .is_file());
        assert!(!f
            .dst
            .join("Profiles/새프로필/RazorEnhanced.settings.AUTOLOOT_ITEMS")
            .exists());
        let value = read_json(
            &f.dst
                .join("Profiles/새프로필/RazorEnhanced.settings.SCRIPTING"),
        )
        .unwrap();
        assert_eq!(
            value[0]["FullPath"],
            display_path(&fs::canonicalize(f.dst.join("Scripts/sub/main.py")).unwrap())
        );
        assert_eq!(value[0]["Hotkey"], 123);
        assert_eq!(value[1]["FullPath"], "D:\\external\\keep.py");
        let index = read_json(&f.dst.join("Profiles/RazorEnhanced.NewProfiles")).unwrap();
        assert_eq!(index[0]["Last"], true);
        assert_eq!(index[1]["Last"], false);
        assert_eq!(index[1]["Players"][0]["PlayerSerial"], 42);
    }

    #[test]
    fn ca_merges_only_selected_sections_and_keeps_existing_by_default() {
        let f = Fixture::new(Kind::Ca);
        f.source("Profiles/settings.json",r#"{"Name":"settings.json","Hotkeys":[2],"Macros":{"FilePath":"old-source.py"},"General":{"x":2}}"#);
        let original = r#"{"Name":"settings.json","Hash":"original","Hotkeys":[1],"Macros":{"FilePath":"target-only.py"},"General":{"x":1}}"#;
        f.dest("Profiles/settings.json", original);
        let mut r = f.request(Kind::Ca, &["profiles/settings.json::Hotkeys"], false);
        assert_eq!(f.run(&r).copied, 0);
        assert_eq!(
            fs::read_to_string(f.dst.join("Profiles/settings.json")).unwrap(),
            original
        );
        r.replace = true;
        assert_eq!(f.run(&r).copied, 1);
        let v = read_json(&f.dst.join("Profiles/settings.json")).unwrap();
        assert_eq!(v["Hotkeys"], json!([2]));
        assert_eq!(v["Macros"]["FilePath"], "target-only.py");
        assert_eq!(v["General"]["x"], 1);
    }

    #[test]
    fn preview_detects_changes_and_rejects_arbitrary_selection_and_overlap() {
        let f = Fixture::new(Kind::Re);
        f.source("Scripts/test.py", "one");
        let r = f.request(Kind::Re, &["Scripts/test.py"], false);
        let a = preview(&r).unwrap().fingerprint;
        f.source("Scripts/test.py", "two");
        let b = preview(&r).unwrap().fingerprint;
        assert_ne!(a, b);
        f.dest("Scripts/test.py", "existing");
        assert_ne!(b, preview(&r).unwrap().fingerprint);
        let mut invalid = r.clone();
        invalid.selected = vec!["../../settings.json".into()];
        assert!(preview(&invalid).is_err());
        invalid = r;
        invalid.destination = invalid.source.clone();
        assert!(scan(&invalid).is_err());
    }

    #[test]
    fn partial_copy_failure_rolls_back_replacements_and_new_files() {
        let f = Fixture::new(Kind::Re);
        f.source("Scripts/a.py", "new a");
        f.source("Scripts/b.py", "new b");
        f.source("Scripts/c.py", "new c");
        f.dest("Scripts/a.py", "old a");
        let r = f.request(
            Kind::Re,
            &["Scripts/a.py", "Scripts/b.py", "Scripts/c.py"],
            true,
        );
        let error = execute_with_hook(plan(&r).unwrap(), &f.root.join("backups"), |i| {
            if i == 2 {
                Err("simulated IO failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(error.contains("변경 파일 복구 완료"));
        assert_eq!(
            fs::read_to_string(f.dst.join("Scripts/a.py")).unwrap(),
            "old a"
        );
        assert!(!f.dst.join("Scripts/b.py").exists());
        assert!(!f.dst.join("Scripts/c.py").exists());
    }

    #[test]
    fn custom_profile_paths_are_respected() {
        let f = Fixture::new(Kind::Ca);
        f.source(
            "Assistant.json",
            r#"{"ProfileDirectory":"CustomProfiles","GlobalDirectory":"CustomData"}"#,
        );
        f.dest(
            "Assistant.json",
            r#"{"ProfileDirectory":"OtherProfiles","GlobalDirectory":"OtherData"}"#,
        );
        f.source("CustomProfiles/test.json", r#"{"Hotkeys":[1]}"#);
        f.source("CustomData/Macros/test.py", "pass");
        let r = f.request(
            Kind::Ca,
            &["profiles/test.json::Hotkeys", "Macros/test.py"],
            false,
        );
        f.run(&r);
        assert!(f.dst.join("OtherProfiles/test.json").is_file());
        assert!(f.dst.join("OtherData/Macros/test.py").is_file());
        assert!(!f.dst.join("Profiles").exists());
    }

    #[test]
    fn external_script_selection_copies_exact_file_and_updates_re_reference() {
        let f = Fixture::new(Kind::Re);
        let external = f.root.join("custom-scripts").join("main.py");
        write(&external, "print('external')");
        write(&f.root.join("custom-scripts/unselected.py"), "do not copy");
        f.source("Profiles/demo/RazorEnhanced.settings.GENERAL", "[]");
        f.source(
            "Profiles/demo/RazorEnhanced.settings.SCRIPTING",
            &json!([{"Filename":"main.py","FullPath":display_path(&external)}]).to_string(),
        );
        let mut r = f.request(
            Kind::Re,
            &["profiles/demo/RazorEnhanced.settings.SCRIPTING"],
            false,
        );
        let items = scan(&r).unwrap().items;
        let found: Vec<_> = items
            .iter()
            .filter(|i| i.id.starts_with("external/"))
            .collect();
        assert_eq!(found.len(), 1);
        r.selected.push(found[0].id.clone());
        let result = f.run(&r);
        assert_eq!(result.copied, 4);
        let value =
            read_json(&f.dst.join("Profiles/demo/RazorEnhanced.settings.SCRIPTING")).unwrap();
        let migrated = Path::new(value[0]["FullPath"].as_str().unwrap());
        assert!(migrated.starts_with(&f.dst));
        assert_eq!(fs::read_to_string(migrated).unwrap(), "print('external')");
        assert!(!migrated.parent().unwrap().join("unselected.py").exists());
        assert_eq!(fs::read_to_string(external).unwrap(), "print('external')");
        assert!(!result
            .notes
            .iter()
            .any(|n| n.contains("스크립트 경로 유지")));
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_rejected() {
        let f = Fixture::new(Kind::Re);
        fs::create_dir_all(f.src.join("Scripts")).unwrap();
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(f.src.join("Scripts").join("link"))
            .arg(&f.dst)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture creation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(scan(&f.request(Kind::Re, &[], false)).is_err());
        fs::remove_dir(f.src.join("Scripts/link")).unwrap();
    }

    #[test]
    #[ignore = "read-only installed source smoke test: GGO_IMPORT_SOURCE and GGO_IMPORT_KIND"]
    fn installed_profiles_copy_into_disposable_destination() {
        let kind = match std::env::var("GGO_IMPORT_KIND").unwrap().as_str() {
            "cuo" => Kind::Cuo,
            "re" => Kind::Re,
            "ca" => Kind::Ca,
            _ => panic!("invalid kind"),
        };
        let f = Fixture::new(kind);
        let mut r = f.request(kind, &[], false);
        r.source = std::env::var("GGO_IMPORT_SOURCE").unwrap();
        let scanned = scan(&r).unwrap();
        r.selected = scanned
            .items
            .iter()
            .filter(|i| match kind {
                Kind::Cuo => i.id.ends_with("/profile.json") || i.id.ends_with("/macros.xml"),
                Kind::Re => {
                    i.id.starts_with("profiles/default/")
                        && (i.id.ends_with(".GENERAL")
                            || i.id.ends_with(".HOTKEYS")
                            || i.id.ends_with(".SCRIPTING"))
                }
                Kind::Ca => {
                    i.id == "profiles/settings.json::Hotkeys"
                        || i.id == "profiles/settings.json::Macros"
                        || i.id == "profiles/settings.json::General"
                }
            })
            .take(6)
            .map(|i| i.id.clone())
            .collect();
        assert!(
            r.selected.len() >= 2,
            "insufficient installed fixture coverage"
        );
        let (src, dst) = paired(&r).unwrap();
        let candidates = inventory(&r, &src, &dst).unwrap();
        if kind == Kind::Re {
            let mut references = BTreeSet::new();
            script_references(&read_json(&src.profiles.join("default/RazorEnhanced.settings.SCRIPTING")).unwrap(), &mut references);
            for reference in references {
                let file = fs::canonicalize(reference).expect("installed script fixture must exist");
                let c = candidates.iter().find(|c| fs::canonicalize(&c.source).unwrap() == file).expect("referenced script must be selectable");
                r.selected.push(c.item.id.clone());
            }
        }
        let before: Vec<_> = candidates
            .iter()
            .filter(|c| r.selected.contains(&c.item.id))
            .map(|c| (c.source.clone(), hash(&fs::read(&c.source).unwrap())))
            .collect();
        let preview = preview(&r).unwrap();
        let result = f.run(&r);
        assert!(result.copied > 0);
        if kind == Kind::Re { assert!(!result.notes.iter().any(|n| n.contains("스크립트 경로 유지"))); }
        for (path, h) in before {
            assert_eq!(hash(&fs::read(path).unwrap()), h, "source modified");
        }
        for c in preview.changes {
            assert!(Path::new(&c.path).is_file());
        }
        println!("Installed {:?}: scanned {} items, selected {}, verified {} destination files; source hashes unchanged; external references reported: {}",kind,scanned.items.len(),r.selected.len(),result.copied,result.notes.iter().filter(|n|n.contains("스크립트 경로 유지")).count());
    }
}
