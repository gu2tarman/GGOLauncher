//! DPAPI 기반 비밀번호 암호화.
//!
//! 저장 포맷:
//!   - `dpapi:<base64>` → DPAPI로 암호화된 데이터 (같은 OS 사용자만 복호화 가능)
//!   - 그 외 → 레거시 평문 (읽기 전용. 다음 저장 시 암호화로 마이그레이션됨)
//!
//! 보안 원칙: encrypt 실패 시 절대 평문 fallback 하지 않음(Err 반환).

use base64::Engine;

const PREFIX: &str = "dpapi:";

/// 저장된 문자열에서 평문 추출. 평문이면 그대로 반환 (레거시 read-only 마이그레이션).
pub fn decrypt_or_passthrough(stored: &str) -> String {
    if stored.is_empty() {
        return String::new();
    }
    if let Some(b64) = stored.strip_prefix(PREFIX) {
        match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(blob) => match dpapi_decrypt(&blob) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) => {
                    eprintln!("[dpapi decrypt] 실패: {e} — 빈 문자열 반환");
                    String::new()
                }
            },
            Err(e) => {
                eprintln!("[base64 decode] 실패: {e}");
                String::new()
            }
        }
    } else {
        // 평문 (레거시)
        stored.to_string()
    }
}

/// 평문 → "dpapi:<base64>" 형식. 빈 문자열은 빈 그대로(Ok).
/// DPAPI 실패 시 Err — 호출자는 저장을 중단해야 함(평문 저장 금지).
pub fn encrypt(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    let blob = dpapi_encrypt(plain.as_bytes())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&blob);
    Ok(format!("{PREFIX}{b64}"))
}

// ── Windows DPAPI ────────────────────────────────────────────
#[cfg(windows)]
fn dpapi_encrypt(plain: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut in_blob,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err("CryptProtectData 실패".to_string());
    }
    let result =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe { LocalFree(out_blob.pbData as _) };
    Ok(result)
}

#[cfg(windows)]
fn dpapi_decrypt(blob: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err("CryptUnprotectData 실패 (다른 사용자/PC?)".to_string());
    }
    let result =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe { LocalFree(out_blob.pbData as _) };
    Ok(result)
}

#[cfg(not(windows))]
fn dpapi_encrypt(_plain: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI는 Windows 전용".to_string())
}

#[cfg(not(windows))]
fn dpapi_decrypt(_blob: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI는 Windows 전용".to_string())
}
