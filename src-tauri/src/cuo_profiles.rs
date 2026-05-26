use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct CuoProfileCandidate {
    pub account: String,
    pub characters: Vec<String>,
}

pub fn list(cuo_path: &str) -> Vec<CuoProfileCandidate> {
    let profiles_dir = PathBuf::from(cuo_path).join("Data").join("Profiles");
    let Ok(entries) = std::fs::read_dir(&profiles_dir) else {
        return Vec::new();
    };

    let mut accounts: Vec<CuoProfileCandidate> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }

            let account = entry.file_name().to_string_lossy().trim().to_string();
            if account.is_empty() {
                return None;
            }

            let mut characters = list_character_dirs(entry.path());
            characters.sort_by_key(|s| s.to_lowercase());
            characters.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());

            Some(CuoProfileCandidate {
                account,
                characters,
            })
        })
        .collect();

    accounts.sort_by_key(|c| c.account.to_lowercase());
    accounts
}

fn list_character_dirs(account_dir: PathBuf) -> Vec<String> {
    let Ok(server_entries) = std::fs::read_dir(account_dir) else {
        return Vec::new();
    };

    server_entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .flat_map(|server_entry| {
            std::fs::read_dir(server_entry.path())
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
        })
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().trim().to_string();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}
