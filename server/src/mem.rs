//! RAM de uma árvore de processos lida do /proc (Linux). PSS divide as páginas compartilhadas entre quem as
//! usa, então a soma das 4 contas bate com o que a VPS realmente gasta. Fora do Linux devolve `None`.
use std::collections::HashMap;
use std::fs;

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

/// MB usados por `root` e todos os descendentes
pub fn tree_mb(root: u32) -> Option<u64> {
    if root == 0 || !std::path::Path::new("/proc/self/stat").exists() {
        return None;
    }
    let parents = parents();
    let mut tree = vec![root];
    let mut i = 0;
    while i < tree.len() {
        let pid = tree[i];
        tree.extend(parents.iter().filter(|(_, &pp)| pp == pid).map(|(&p, _)| p));
        i += 1;
    }
    Some(tree.iter().map(|&p| pss_kb(p)).sum::<u64>() / 1024)
}
