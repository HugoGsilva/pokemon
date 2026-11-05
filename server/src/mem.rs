//! RAM de uma árvore de processos. No Linux vem do /proc: PSS divide as páginas compartilhadas entre quem as
//! usa, então a soma das 4 contas bate com o que a VPS realmente gasta. No Windows é a memória privada de cada
//! processo (ver `windows::tree_mb`). Em outros sistemas devolve `None`.
use std::collections::HashMap;
#[cfg(not(windows))]
use std::fs;

#[cfg(not(windows))]
/// Pai de cada processo vivo. O nome do processo (entre parênteses) pode ter espaço: o pai vem depois do último ')'
fn parents() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let Ok(dir) = fs::read_dir("/proc") else { return map };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) else { continue };
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else { continue };
        if let Some(ppid) = rest.split_whitespace().nth(1).and_then(|p| p.parse().ok()) {
            map.insert(pid, ppid);
        }
    }
    map
}

#[cfg(not(windows))]
fn pss_kb(pid: u32) -> u64 {
    let field = |text: &str, key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l[key.len()..].split_whitespace().next()?.parse::<u64>().ok())
    };
    if let Some(kb) = fs::read_to_string(format!("/proc/{}/smaps_rollup", pid)).ok().and_then(|t| field(&t, "Pss:")) {
        return kb;
    }
    fs::read_to_string(format!("/proc/{}/status", pid)).ok().and_then(|t| field(&t, "VmRSS:")).unwrap_or(0)
}

/// `root` e todos os descendentes
fn descendants(root: u32, parents: &HashMap<u32, u32>) -> Vec<u32> {
    let mut tree = vec![root];
    let mut i = 0;
    while i < tree.len() {
        let pid = tree[i];
        tree.extend(parents.iter().filter(|(&p, &pp)| pp == pid && p != pid).map(|(&p, _)| p));
        i += 1;
    }
    tree
}

/// MB usados por `root` e todos os descendentes
#[cfg(not(windows))]
pub fn tree_mb(root: u32) -> Option<u64> {
    if root == 0 || !std::path::Path::new("/proc/self/stat").exists() {
        return None;
    }
    Some(descendants(root, &parents()).iter().map(|&p| pss_kb(p)).sum::<u64>() / 1024)
}

#[cfg(windows)]
pub use windows::tree_mb;

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};

    fn parents() -> HashMap<u32, u32> {
        let mut map = HashMap::new();
        // SAFETY: snapshot lido com a estrutura do tamanho certo e fechado no fim
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return map;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry) != 0;
            while ok {
                map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                ok = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        map
    }

    /// Memória privada (commit) do processo, em KB: a mesma ordem de grandeza da coluna "Memória" do
    /// Gerenciador de Tarefas, sem contar duas vezes o que os processos compartilham
    fn private_kb(pid: u32) -> u64 {
        // SAFETY: handle aberto só para consulta e fechado em seguida
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return 0;
            }
            let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            let ok = K32GetProcessMemoryInfo(handle, &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS, size);
            CloseHandle(handle);
            if ok == 0 {
                0
            } else {
                counters.PrivateUsage as u64 / 1024
            }
        }
    }

    pub fn tree_mb(root: u32) -> Option<u64> {
        if root == 0 {
            return None;
        }
        Some(super::descendants(root, &parents()).iter().map(|&p| private_kb(p)).sum::<u64>() / 1024)
    }
}
