use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    pub id: u8,
    pub user: String,
    pub pass: String,
}

#[cfg(target_os = "windows")]
mod win_dpapi {
    use std::ptr;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        unsafe {
            if CryptProtectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .is_ok()
            {
                let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
                let result = slice.to_vec();
                let _ = LocalFree(windows::Win32::Foundation::HLOCAL(out_blob.pbData as _));
                Ok(result)
            } else {
                Err("Falha na chamada CryptProtectData da DPAPI".to_string())
            }
        }
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        unsafe {
            if CryptUnprotectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .is_ok()
            {
                let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
                let result = slice.to_vec();
                let _ = LocalFree(windows::Win32::Foundation::HLOCAL(out_blob.pbData as _));
                Ok(result)
            } else {
                Err("Falha na chamada CryptUnprotectData da DPAPI".to_string())
            }
        }
    }
}

// Sem DPAPI fora do Windows: falha explícita em vez de gravar senhas em texto puro
#[cfg(not(target_os = "windows"))]
mod win_dpapi {
    const UNSUPPORTED: &str = "Armazenamento seguro de credenciais disponível apenas no Windows (DPAPI)";

    pub fn protect(_data: &[u8]) -> Result<Vec<u8>, String> {
        Err(UNSUPPORTED.to_string())
    }
    pub fn unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
        Err(UNSUPPORTED.to_string())
    }
}

pub fn save_accounts(file_path: PathBuf, accounts: &[Account]) -> Result<(), String> {
    let json_bytes = serde_json::to_vec(accounts).map_err(|e| e.to_string())?;
    let encrypted = win_dpapi::protect(&json_bytes)?;

    let tmp_path = file_path.with_extension("enc.tmp");
    fs::write(&tmp_path, encrypted).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &file_path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_accounts(file_path: PathBuf) -> Result<Vec<Account>, String> {
    if !file_path.exists() {
        return Ok(Vec::new());
    }
    let encrypted = fs::read(&file_path).map_err(|e| e.to_string())?;
    let decrypted = win_dpapi::unprotect(&encrypted)?;
    let accounts: Vec<Account> = serde_json::from_slice(&decrypted).map_err(|e| e.to_string())?;
    Ok(accounts)
}
