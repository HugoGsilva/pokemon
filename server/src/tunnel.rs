//! Túnel até a VPS, para quando as contas rodam no PC: o `chisel` conecta no servidor chisel da VPS (por HTTPS,
//! a conexão sai do PC, sem abrir porta no roteador) e pede para ela repassar a porta do domínio para cá.
//! O jogo continua vendo o IP de casa; a VPS só entrega a página. Cai e volta sozinho.
use crate::config::{exe_dir, Config};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

fn find_chisel(cfg: &Config) -> PathBuf {
    if let Some(bin) = &cfg.tunnel_bin {
        return bin.into();
    }
    let local = exe_dir().join(if cfg!(windows) { "chisel.exe" } else { "chisel" });
    if local.is_file() {
        local
    } else {
        "chisel".into()
    }
}

/// Para onde a VPS manda as conexões: a porta do servidor web, vista de dentro do PC
fn local_target(bind: &str) -> String {
    match bind.rsplit_once(':') {
        Some((host, port)) if host == "0.0.0.0" || host == "[::]" => format!("127.0.0.1:{}", port),
        _ => bind.to_string(),
    }
}

fn log_lines(stream: impl AsyncRead + Unpin + Send + 'static) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[túnel] {}", line.trim());
        }
    });
}

pub fn spawn(cfg: Arc<Config>) {
    let (Some(url), Some(auth)) = (cfg.tunnel_url.clone(), cfg.tunnel_auth.clone()) else { return };
    let bin = find_chisel(&cfg);
    let remote = format!("R:{}:{}", cfg.tunnel_remote, local_target(&cfg.bind));
    tokio::spawn(async move {
        loop {
            eprintln!("[túnel] conectando em {}", url);
            // A senha vai pelo ambiente (AUTH), não pela linha de comando
            let child = Command::new(&bin)
                .args(["client", "--keepalive", "25s", "--max-retry-interval", "30s", &url, &remote])
                .env("AUTH", &auth)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn();
            match child {
                Ok(mut child) => {
                    if let Some(out) = child.stdout.take() {
                        log_lines(out);
                    }
                    if let Some(err) = child.stderr.take() {
                        log_lines(err);
                    }
                    match child.wait().await {
                        Ok(status) => eprintln!("[túnel] chisel saiu ({}); de novo em 5 s", status),
                        Err(err) => eprintln!("[túnel] {}; de novo em 5 s", err),
                    }
                }
                Err(err) => eprintln!("[túnel] não foi possível iniciar {}: {}", bin.display(), err),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::local_target;

    #[test]
    fn local_target_points_inside() {
        assert_eq!(local_target("0.0.0.0:8080"), "127.0.0.1:8080");
        assert_eq!(local_target("127.0.0.1:9000"), "127.0.0.1:9000");
    }
}
