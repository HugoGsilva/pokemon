//! RAM total do app: o próprio processo + todos os descendentes (as árvores de processos do WebView2).
//! Usa o conjunto de trabalho privado, a mesma métrica da coluna "Memória" do Gerenciador de Tarefas.

#[cfg(windows)]
pub fn app_memory_bytes() -> Result<u64, String> {
    use std::collections::{HashMap, HashSet};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    // Mapa pai -> filhos de todos os processos do sistema
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    unsafe {
        let snapshot =
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            children
                .entry(entry.th32ParentProcessID)
                .or_default()
                .push(entry.th32ProcessID);
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }

    let mut total = 0u64;
    let mut seen = HashSet::new();
    let mut stack = vec![std::process::id()];
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids);
        }
        unsafe {
            let Ok(handle) =
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
            else {
                continue;
            };
            let mut counters = PROCESS_MEMORY_COUNTERS_EX2 {
                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
                ..Default::default()
            };
            let read = K32GetProcessMemoryInfo(
                handle,
                &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                counters.cb,
            );
            if read.as_bool() {
                total += counters.PrivateWorkingSetSize as u64;
            }
            let _ = CloseHandle(handle);
        }
    }
    Ok(total)
}

#[cfg(not(windows))]
pub fn app_memory_bytes() -> Result<u64, String> {
    Err("Medição de RAM disponível apenas no Windows".into())
}
