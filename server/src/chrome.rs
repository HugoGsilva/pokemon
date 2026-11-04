//! Um Chromium headless por conta, cada um com o seu perfil em disco (login e cookies sobrevivem a reinícios).
//! Processos separados custam ~0,1 GB a mais que abas num navegador só, mas dão a RAM exata de cada conta
//! (o vigia de memória precisa disso) e uma conta que trava não derruba as outras.
use crate::config::Config;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Binários procurados quando `PG_CHROME` não está definido (o primeiro é o do Debian, usado no Docker)
const CANDIDATES: [&str; 5] =
    ["chromium-headless-shell", "chrome-headless-shell", "chromium", "google-chrome", "msedge"];

pub struct Chrome {
    pub child: Child,
    pub pid: u32,
    pub ws_url: String,
}

pub fn profile_dir(cfg: &Config, slot: u8) -> PathBuf {
    cfg.data_dir.join(format!("conta{}", slot))
}

fn find_binary(cfg: &Config) -> String {
    if let Some(bin) = &cfg.chrome {
        return bin.clone();
    }
    for fixed in ["/usr/lib/chromium/chromium-headless-shell", "/usr/lib/chromium/headless_shell"] {
        if Path::new(fixed).is_file() {
            return fixed.to_string();
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for name in CANDIDATES {
        for dir in std::env::split_paths(&path) {
            if dir.join(name).is_file() {
                return dir.join(name).display().to_string();
            }
        }
    }
    CANDIDATES[0].to_string()
}

/// Travas que o Chromium deixa quando é morto: com outro hostname (o container muda a cada recriação) ele se
/// recusa a abrir o perfil, achando que outro computador está usando
fn clear_locks(dir: &Path) {
    for name in ["SingletonLock", "SingletonCookie", "SingletonSocket", "DevToolsActivePort"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

pub async fn launch(cfg: &Config, slot: u8) -> Result<Chrome, String> {
    let dir = profile_dir(cfg, slot);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    clear_locks(&dir);

    let bin = find_binary(cfg);
    let mut cmd = Command::new(&bin);
    cmd.arg(format!("--user-data-dir={}", dir.display()))
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--remote-debugging-port=0")
        .arg(format!("--window-size={},{}", cfg.width, cfg.height))
        .arg(format!("--js-flags=--max-old-space-size={}", cfg.js_heap_mb))
        .args([
            "--headless",
            "--no-first-run",
            "--no-default-browser-check",
            // Sem placa de vídeo na VPS: tudo em software, sem o processo de GPU segurando texturas
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--disable-extensions",
            "--disable-component-update",
            "--disable-sync",
            "--disable-breakpad",
            "--disable-domain-reliability",
            "--disable-client-side-phishing-detection",
            "--disable-default-apps",
            "--metrics-recording-only",
            "--no-pings",
            "--mute-audio",
            "--password-store=basic",
            "--renderer-process-limit=2",
            // O jogo tem que seguir rodando sem ninguém olhando
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--disable-backgrounding-occluded-windows",
            "--disable-features=Translate,BackForwardCache,MediaRouter,OptimizationHints,AutofillServerCommunication,CalculateNativeWinOcclusion,HeavyAdIntervention",
            "--disk-cache-size=67108864",
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // O sandbox do Chromium precisa de privilégios que o container não tem; o isolamento fica com o Docker
    // (usuário sem root, só o site do jogo é aberto)
    if std::env::var("PG_NO_SANDBOX").map(|v| v != "0").unwrap_or(cfg!(target_os = "linux")) {
        cmd.arg("--no-sandbox");
    }

    let mut child = cmd.spawn().map_err(|e| format!("Não foi possível iniciar {}: {}", bin, e))?;
    let pid = child.id().unwrap_or(0);

    // Porta 0: o Chromium escolhe uma livre e avisa no stderr ("DevTools listening on ws://...").
    // O resto do stderr é lido e descartado, para o pipe não encher e travar o navegador
    let (url_tx, url_rx) = tokio::sync::oneshot::channel();
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut url_tx = Some(url_tx);
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(at) = line.find("ws://") {
                    if let Some(tx) = url_tx.take() {
                        let _ = tx.send(line[at..].trim().to_string());
                    }
                }
            }
        });
    }
    match tokio::time::timeout(Duration::from_secs(20), url_rx).await {
        Ok(Ok(ws_url)) => Ok(Chrome { child, pid, ws_url }),
        _ => {
            let status = child.try_wait().ok().flatten();
            let _ = child.kill().await;
            Err(match status {
                Some(status) => format!("{} saiu ao iniciar ({})", bin, status),
                None => format!("{} não abriu o DevTools em 20s", bin),
            })
        }
    }
}
