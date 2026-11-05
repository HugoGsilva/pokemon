//! PokeGrid Remoto: as contas rodam num Chromium headless (na VPS ou no PC) e a página (no seu domínio) mostra
//! a tela de cada uma, repassa os seus cliques e o teclado e traz o painel do Modo Eco. No PC, um túnel até a
//! VPS leva o domínio até aqui. Ver `server/README.md`.
mod account;
mod cdp;
mod chrome;
mod config;
mod mem;
mod stats;
mod tunnel;
mod web;

// A contabilidade do painel é a mesma do app Windows (código compartilhado, sem cópia)
#[allow(dead_code)]
#[path = "../../src-tauri/src/domain/catalog.rs"]
mod catalog;
#[allow(dead_code)]
#[path = "../../src-tauri/src/domain/farm.rs"]
mod farm;

use std::sync::Arc;

#[tokio::main]
async fn main() {
    let cfg = match config::Config::from_env() {
        Ok(cfg) => Arc::new(cfg),
        Err(err) => {
            eprintln!("[config] {}", err);
            std::process::exit(2);
        }
    };
    if let Err(err) = std::fs::create_dir_all(&cfg.data_dir) {
        eprintln!("[config] {}: {}", cfg.data_dir.display(), err);
        std::process::exit(2);
    }
    if cfg.password.is_none() {
        eprintln!("[config] ATENÇÃO: sem senha (PG_NO_AUTH=1). Não exponha esta porta na internet.");
    }
    #[cfg(windows)]
    close_children_with_server();

    let stats = stats::Stats::load(&cfg.data_dir, &cfg.game_url);
    stats.spawn_background();
    let video = account::Video::new();
    let accounts: Vec<_> = (1..=cfg.accounts as u8)
        .map(|slot| account::spawn(slot, cfg.clone(), stats.clone(), video.clone()))
        .collect();
    account::spawn_watchdog(cfg.clone(), accounts.iter().map(|a| (a.cmd.clone(), a.status.clone())).collect());

    let app = web::App::new(cfg.clone(), stats.clone(), video, accounts);
    let listener = match tokio::net::TcpListener::bind(&cfg.bind).await {
        Ok(l) => l,
        Err(err) => {
            eprintln!("[web] {}: {}", cfg.bind, err);
            std::process::exit(2);
        }
    };
    eprintln!("[web] ouvindo em {} ({} contas, tela {}x{})", cfg.bind, cfg.accounts, cfg.width, cfg.height);
    tunnel::spawn(cfg.clone());
    let served = axum::serve(listener, web::router(app)).with_graceful_shutdown(shutdown(stats.clone())).await;
    if let Err(err) = served {
        eprintln!("[web] {}", err);
    }
    // Desligando (docker stop): grava o histórico antes de sair; os Chromium morrem junto com o container
    stats.save();
}

/// No Windows, filhos não morrem com o pai: o servidor entra num job que, ao fechar (servidor encerrado ou
/// janela fechada), derruba junto os navegadores e o túnel
#[cfg(windows)]
fn close_children_with_server() {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: o handle do job fica aberto até o processo acabar (é isso que fecha os filhos)
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            eprintln!("[config] sem job do Windows: feche os navegadores à mão se o servidor cair");
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        let set = SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, size);
        if set == 0 || AssignProcessToJobObject(job, GetCurrentProcess()) == 0 {
            eprintln!("[config] sem job do Windows: feche os navegadores à mão se o servidor cair");
        }
    }
}

/// Espera o pedido para desligar e grava o histórico na hora (com páginas abertas o desligamento gracioso
/// pode demorar; depois de 5 s o processo sai de qualquer jeito)
async fn shutdown(stats: std::sync::Arc<stats::Stats>) {
    wait_signal().await;
    eprintln!("[web] desligando");
    stats.save();
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        std::process::exit(0);
    });
}

async fn wait_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    // Janela do console fechada: o Windows dá poucos segundos antes de encerrar o processo
    #[cfg(windows)]
    let term = async {
        if let Ok(mut s) = tokio::signal::windows::ctrl_close() {
            s.recv().await;
        }
    };
    #[cfg(not(any(unix, windows)))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}
