//! Painel do Modo Eco: leitura passiva do WebSocket do jogo em cada conta.
//!
//! O app NÃO injeta script nem manda mensagem nenhuma ao jogo. Ele liga o domínio `Network` do protocolo
//! DevTools do WebView2 (o mesmo que a aba Rede do DevTools usa) e recebe, do lado nativo, uma cópia dos frames
//! que o servidor já manda para a página. A página não tem como perceber, e nada muda no que ela faz.
//! A contabilidade fica em `domain::farm`.
use crate::domain::farm::{export_csv, local_datetime, pretty_slug, FarmState, History, Prices, Snapshot};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime, Webview};

static STATE: OnceLock<Mutex<FarmState>> = OnceLock::new();
/// `fetched_at` do catálogo cujos preços estão em uso (0 = nenhum ainda)
static PRICES_AT: AtomicU64 = AtomicU64::new(0);
/// Cache do catálogo de versão anterior não tem preço por id: baixa de novo uma vez por abertura do app
static FORCED: AtomicBool = AtomicBool::new(false);
const SAVE_EVERY: Duration = Duration::from_secs(60);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn state() -> &'static Mutex<FarmState> {
    STATE.get_or_init(|| Mutex::new(FarmState::new(History::default(), now_ms())))
}

fn history_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Erro ao obter diretório de dados: {}", e))?;
    Ok(dir.join("farm_history.json"))
}

/// Carrega o histórico salvo e começa a gravação periódica. Chamar antes de criar as webviews das contas
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let history = history_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<History>(&b).ok())
        .unwrap_or_default();
    let _ = STATE.set(Mutex::new(FarmState::new(history, now_ms())));

    let app = app.clone();
    std::thread::spawn(move || {
        refresh_prices(&app);
        loop {
            std::thread::sleep(SAVE_EVERY);
            refresh_prices(&app);
            save(&app);
        }
    });
}

/// Preços de NPC do catálogo público do jogo (o mesmo da janela Ferramentas); rebaixa sozinho quando vence
fn refresh_prices<R: Runtime>(app: &AppHandle<R>) {
    let Ok((mut cat, _)) = crate::tools::catalog(app, false) else { return };
    if cat.item_prices.is_empty() && !FORCED.swap(true, Ordering::Relaxed) {
        let Ok((fresh, _)) = crate::tools::catalog(app, true) else { return };
        cat = fresh;
    }
    if cat.item_prices.is_empty() || PRICES_AT.swap(cat.fetched_at, Ordering::Relaxed) == cat.fetched_at {
        return;
    }
    if let Ok(mut st) = state().lock() {
        st.set_prices(Prices { by_id: cat.item_prices.clone() });
    }
}

/// Grava o histórico se mudou (arquivo temporário + rename, para não corromper num desligamento)
pub fn save<R: Runtime>(app: &AppHandle<R>) {
    let json = {
        let Ok(mut st) = state().lock() else { return };
        if !st.dirty {
            return;
        }
        st.history.prune(now_ms());
        st.dirty = false;
        serde_json::to_vec(&st.history)
    };
    let (Ok(json), Ok(path)) = (json, history_path(app)) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Created,
    Frame,
    Closed,
}

const EVENTS: [(&str, Kind); 3] = [
    ("Network.webSocketCreated", Kind::Created),
    ("Network.webSocketFrameReceived", Kind::Frame),
    ("Network.webSocketClosed", Kind::Closed),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedEvent {
    request_id: String,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameEvent {
    request_id: String,
    response: FramePayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FramePayload {
    #[serde(default)]
    opcode: f64,
    #[serde(default)]
    payload_data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClosedEvent {
    request_id: String,
}

fn on_event(slot: usize, kind: Kind, json: &str) {
    let now = now_ms();
    let Ok(mut st) = state().lock() else { return };
    match kind {
        Kind::Created => {
            if let Ok(e) = serde_json::from_str::<CreatedEvent>(json) {
                st.socket_created(slot, &e.request_id, &e.url);
            }
        }
        Kind::Frame => {
            // Só frames de texto (opcode 1): o jogo manda JSON
            if let Ok(e) = serde_json::from_str::<FrameEvent>(json) {
                if e.response.opcode == 1.0 {
                    st.frame(slot, &e.request_id, &e.response.payload_data, now);
                }
            }
        }
        Kind::Closed => {
            if let Ok(e) = serde_json::from_str::<ClosedEvent>(json) {
                st.socket_closed(slot, &e.request_id, now);
            }
        }
    }
}

/// Começa a ler os frames do WebSocket da conta `slot` (1..=4). Só leitura: nada roda na página
#[cfg(windows)]
pub fn attach<R: Runtime>(webview: &Webview<R>, slot: usize) {
    let _ = webview.with_webview(move |platform| unsafe {
        use webview2_com::{take_pwstr, DevToolsProtocolEventReceivedEventHandler};
        use windows_core::{HSTRING, PCWSTR, PWSTR};

        let Ok(core) = platform.controller().CoreWebView2() else { return };
        for (event, kind) in EVENTS {
            let name = HSTRING::from(event);
            let Ok(receiver) = core.GetDevToolsProtocolEventReceiver(PCWSTR(name.as_ptr())) else { continue };
            let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut json = PWSTR::null();
                    if args.ParameterObjectAsJson(&mut json).is_ok() {
                        on_event(slot, kind, &take_pwstr(json));
                    }
                }
                Ok(())
            }));
            let mut token = 0i64;
            let _ = receiver.add_DevToolsProtocolEventReceived(&handler, &mut token);
        }
        enable_network(&core, true);
    });
}

#[cfg(not(windows))]
pub fn attach<R: Runtime>(_webview: &Webview<R>, _slot: usize) {}

/// Liga de novo o domínio Network depois de uma carga de página (recarregar, renderer que caiu)
#[cfg(windows)]
pub fn reattach<R: Runtime>(webview: &Webview<R>) {
    let _ = webview.with_webview(|platform| unsafe {
        if let Ok(core) = platform.controller().CoreWebView2() {
            enable_network(&core, true);
        }
    });
}

#[cfg(not(windows))]
pub fn reattach<R: Runtime>(_webview: &Webview<R>) {}

/// `Network.enable` sem guardar corpo de resposta nenhum (buffers 0): só os eventos, sem RAM extra.
/// Se o runtime recusar os buffers, tenta sem parâmetros
#[cfg(windows)]
unsafe fn enable_network(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, first: bool) {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::{HSTRING, PCWSTR};

    let method = HSTRING::from("Network.enable");
    let params = HSTRING::from(if first { r#"{"maxTotalBufferSize":0,"maxResourceBufferSize":0}"# } else { "{}" });
    let retry = core.clone();
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
        if result.is_err() && first {
            unsafe { enable_network(&retry, false) };
        }
        Ok(())
    }));
    let _ = core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &done);
}

/// Nome da hunt pelo catálogo; sem catálogo, o slug com espaços
fn hunt_name(slug: &str) -> String {
    if slug.is_empty() {
        return String::new();
    }
    crate::tools::catalog_loaded()
        .and_then(|c| c.hunts.iter().find(|h| h.slug == slug).map(|h| h.name.clone()))
        .unwrap_or_else(|| pretty_slug(slug))
}

/// Tudo que o painel do Modo Eco mostra. `tz_offset_min` = `new Date().getTimezoneOffset()` (define o "hoje")
#[tauri::command(async)]
pub fn farm_snapshot(tz_offset_min: i32) -> Result<Snapshot, String> {
    let mut snap = state().lock().map_err(|e| e.to_string())?.snapshot(now_ms(), tz_offset_min);
    for a in &mut snap.accounts {
        a.hunt_name = hunt_name(&a.hunt_slug);
    }
    Ok(snap)
}

/// Zerar: sessão nova em todas as contas; o histórico do dia e as listas continuam
#[tauri::command]
pub fn farm_reset() -> Result<(), String> {
    state().lock().map_err(|e| e.to_string())?.reset_sessions(now_ms());
    Ok(())
}

/// Exporta em CSV (`days` = um dia por linha, `hunts` = uma hunt por linha) para a pasta Downloads e
/// devolve o caminho do arquivo. `names` = nome de cada conta (1..=4) para a coluna Conta
#[tauri::command(async)]
pub fn farm_export(kind: String, tz_offset_min: i32, names: Vec<String>, app: AppHandle) -> Result<String, String> {
    let now = now_ms();
    let account = |a: u8| {
        names
            .get(a as usize - 1)
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Conta {}", a))
    };
    let (label, csv) = {
        let st = state().lock().map_err(|e| e.to_string())?;
        export_csv(&st.history, &kind, now, tz_offset_min, account, hunt_name)?
    };

    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| format!("Erro ao obter a pasta Downloads: {}", e))?;
    let stamp = local_datetime(now, tz_offset_min).replace([' ', ':'], "_");
    let path = dir.join(format!("pokegrid-{}-{}.csv", label, stamp));
    std::fs::write(&path, csv).map_err(|e| format!("Erro ao gravar {}: {}", path.display(), e))?;
    Ok(path.display().to_string())
}
