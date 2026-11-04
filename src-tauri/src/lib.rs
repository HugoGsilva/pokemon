pub mod domain;
pub mod farm;
pub mod metrics;
pub mod security;
pub mod tools;
pub mod webviews;

use domain::iv_math;
use security::dpapi::{self, Account};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use tauri::{
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, Manager, Runtime, Webview, WebviewUrl, WebviewWindow,
};

/// O que ocupa o drawer lateral; o resto do Twitch/Discord fica em Ghost (1x1)
const DRAWER_NONE: u8 = 0;
const DRAWER_TWITCH: u8 = 1;
const DRAWER_DISCORD: u8 = 2;

static DRAWER: AtomicU8 = AtomicU8::new(DRAWER_NONE);
static ECO_ENABLED: AtomicBool = AtomicBool::new(false);
/// Modo Foco: 0 = grade 2x2; 1..=4 = só aquela conta, ampliada
static FOCUS: AtomicU8 = AtomicU8::new(0);
/// Canais oficiais, em ordem de prioridade: a live N assiste o N-ésimo canal da lista que estiver ao vivo
static TWITCH_CHANNELS: Mutex<Vec<String>> = Mutex::new(Vec::new());

// Regras do jogo (https://poke.idleworld.online/rules):
// - mais de 4 contas por jogador leva a exclusão permanente;
// - scripts, extensões, macros e automações sem permissão da staff são proibidos.
// Por isso as webviews do jogo NÃO recebem nenhum script injetado: o app só exibe a interface oficial.
const MAX_ACCOUNTS: usize = 4;
const GAME_URL: &str = "https://poke.idleworld.online";
// Página mínima de www.twitch.tv onde a live espera enquanto não há canal da lista ao vivo para ela
// (o script precisa da origem da Twitch para consultar quem está ao vivo)
const TWITCH_IDLE_URL: &str = "https://www.twitch.tv/robots.txt";
/// Máximo de lives que contam para o bônus ao mesmo tempo
const TWITCH_LIVES: usize = 5;
const DISCORD_URL: &str = "https://discord.com/app";

/// Flags do WebView2 para as webviews filhas (o `additionalBrowserArgs` do tauri.conf.json só vale para a `main`).
/// Inclui os `--disable-features` padrão do wry, que são descartados quando passamos flags próprias.
/// `SpareRendererForSitePerProcess` corta o renderer "reserva" em cada uma das 5 árvores de processos do WebView2.
/// NÃO usar `--disable-site-isolation-trials`: o captcha do Cloudflare (Turnstile) no login do jogo detecta
/// o isolamento desligado e retorna "Falha na verificação".
const BROWSER_ARGS: &str = "--disable-sync --disable-extensions --disable-component-update --disable-background-networking --disable-features=Translate,OptimizationHints,MediaRouter,SpareRendererForSitePerProcess,IntensiveWakeUpThrottling,msWebOOUI,msPdfOOUI,msSmartScreenProtection --disk-cache-size=33554432 --media-cache-size=16777216 --disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --autoplay-policy=no-user-gesture-required";

/// Iframes de anúncio da Twitch (Amazon) são recriados sem nunca serem liberados: em ~10 min
/// o renderer passa de 2 GB e morre com "Out of Memory". Resolver esses hosts para NOTFOUND corta o vazamento.
const TWITCH_AD_HOST_RULES: &str = "--host-resolver-rules=\"MAP *.amazon-adsystem.com ~NOTFOUND, MAP *.advertising.amazon.dev ~NOTFOUND, MAP edge.ads.twitch.tv ~NOTFOUND, MAP *.scorecardresearch.com ~NOTFOUND\"";

fn game_labels() -> impl Iterator<Item = String> {
    (1..=MAX_ACCOUNTS).map(|i| format!("conta{}", i))
}

/// `twitch1` é a live que aparece no drawer; as demais ficam sempre em Ghost
fn twitch_labels() -> impl Iterator<Item = String> {
    (1..=TWITCH_LIVES).map(|i| format!("twitch{}", i))
}

/// Webview que o drawer está exibindo agora (se houver)
fn drawer_label() -> Option<&'static str> {
    match DRAWER.load(Ordering::Relaxed) {
        DRAWER_TWITCH => Some("twitch1"),
        DRAWER_DISCORD => Some("discord"),
        _ => None,
    }
}

fn twitch_ghost_script(drawer_open: bool) -> String {
    format!(
        "if (window.applyTwitchGhostMode) window.applyTwitchGhostMode({});",
        !drawer_open
    )
}

/// Entrega a lista de canais à página e pede uma verificação imediata (usado no load e ao salvar a lista)
fn twitch_channels_script() -> String {
    let channels = TWITCH_CHANNELS.lock().map(|c| c.clone()).unwrap_or_default();
    format!(
        "window.__PG_TWITCH_CHANNELS = {};\nif (window.__pgCheckLives) window.__pgCheckLives();",
        serde_json::Value::from(channels)
    )
}

fn channels_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Erro ao obter diretório de dados: {}", e))?;
    Ok(app_dir.join("twitch_channels.json"))
}

/// Pede ao WebView2 para liberar caches e reduzir RAM (MemoryUsageTargetLevel, indicado para webviews inativas)
#[cfg(windows)]
fn set_low_memory(webview: &Webview, low: bool) {
    let _ = webview.with_webview(move |platform| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        };
        use windows_core::Interface;

        // 0 = Normal, 1 = Low
        let level = COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL(if low { 1 } else { 0 });
        if let Ok(core) = platform.controller().CoreWebView2() {
            // Runtimes antigos do WebView2 não têm ICoreWebView2_19: ignora silenciosamente
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                let _ = core19.SetMemoryUsageTargetLevel(level);
            }
        }
    });
}

#[cfg(not(windows))]
fn set_low_memory(_webview: &Webview, _low: bool) {}

fn accounts_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Erro ao obter diretório de dados: {}", e))?;
    Ok(app_dir.join("accounts.enc"))
}

/// Tamanho lógico atual da janela principal
fn main_logical_size<R: Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    // `get_webview_window` retorna None depois que a janela ganha webviews filhas
    let window = app.get_window("main")?;
    let size = window.inner_size().ok()?;
    let logical = size.to_logical::<f64>(window.scale_factor().unwrap_or(1.0));
    Some((logical.width, logical.height))
}

/// Reposiciona os 4 quadrantes e a webview da Twitch
fn layout_webviews<R: Runtime>(app: &AppHandle<R>, width: f64, height: f64) {
    // Janela minimizada reporta 0x0
    if width < 1.0 || height < 1.0 {
        return;
    }

    let open = drawer_label();
    let quadrants = webviews::manager::calculate_quadrants(width, height);
    let focus = FOCUS.load(Ordering::Relaxed) as usize;
    for (i, label) in game_labels().enumerate() {
        if let Some(wv) = app.get_webview(&label) {
            let b = match focus {
                0 => quadrants[i],
                f if f == i + 1 => webviews::manager::calculate_focus_bounds(width, height, open.is_some()),
                _ => webviews::manager::calculate_offscreen_bounds(quadrants[i]),
            };
            let _ = wv.set_position(b.position);
            let _ = wv.set_size(b.size);
        }
    }

    for label in twitch_labels().chain(std::iter::once("discord".to_string())) {
        if let Some(wv) = app.get_webview(&label) {
            let b = if open == Some(label.as_str()) {
                webviews::manager::calculate_drawer_bounds(width, height, true)
            } else if label.starts_with("twitch") {
                webviews::manager::calculate_twitch_ghost_bounds()
            } else {
                webviews::manager::calculate_drawer_bounds(width, height, false)
            };
            let _ = wv.set_position(b.position);
            let _ = wv.set_size(b.size);
        }
    }
}

/// Modo Eco nativo: oculta as webviews das contas (como minimizar o navegador) e pede ao WebView2
/// para liberar memória. Nada é executado dentro da página do jogo.
#[tauri::command]
fn toggle_eco_mode(enabled: bool, app: AppHandle) -> Result<(), String> {
    ECO_ENABLED.store(enabled, Ordering::Relaxed);

    for label in game_labels() {
        if let Some(wv) = app.get_webview(&label) {
            let result = if enabled { wv.hide() } else { wv.show() };
            result.map_err(|e| format!("{}: {}", label, e))?;
            set_low_memory(&wv, enabled);
        }
    }
    Ok(())
}

/// Abre o drawer com a Twitch (live 1) ou o Discord, ou fecha (`"none"`); quem sai do drawer volta ao Ghost
#[tauri::command]
fn set_drawer(target: String, app: AppHandle) -> Result<(), String> {
    let drawer = match target.as_str() {
        "twitch" => DRAWER_TWITCH,
        "discord" => DRAWER_DISCORD,
        "none" => DRAWER_NONE,
        other => return Err(format!("Drawer desconhecido: {}", other)),
    };
    DRAWER.store(drawer, Ordering::Relaxed);

    if let Some((width, height)) = main_logical_size(&app) {
        layout_webviews(&app, width, height);
    }
    if let Some(tw_wv) = app.get_webview("twitch1") {
        tw_wv
            .eval(twitch_ghost_script(drawer == DRAWER_TWITCH))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Modo Foco: `0` volta à grade 2x2; `1..=4` amplia aquela conta e tira as outras da tela (sem ocultá-las)
#[tauri::command]
fn set_focus(account: u8, app: AppHandle) -> Result<(), String> {
    if account as usize > MAX_ACCOUNTS {
        return Err(format!("Conta inválida: {}", account));
    }
    FOCUS.store(account, Ordering::Relaxed);

    if let Some((width, height)) = main_logical_size(&app) {
        layout_webviews(&app, width, height);
    }
    Ok(())
}

/// Esconde as webviews nativas para que o modal da `main` fique visível (elas são desenhadas por cima do HTML)
#[tauri::command]
fn set_overlay_open(open: bool, app: AppHandle) -> Result<(), String> {
    // Com o Modo Eco ligado as contas já estão ocultas e devem continuar assim ao fechar o modal
    if !ECO_ENABLED.load(Ordering::Relaxed) {
        for label in game_labels() {
            if let Some(wv) = app.get_webview(&label) {
                let result = if open { wv.hide() } else { wv.show() };
                result.map_err(|e| format!("{}: {}", label, e))?;
            }
        }
    }

    // Em Ghost (1x1) as webviews não cobrem o modal; escondê-las pausaria o vídeo das lives
    if let Some(wv) = drawer_label().and_then(|label| app.get_webview(label)) {
        let result = if open { wv.hide() } else { wv.show() };
        result.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn reload_all_accounts(app: AppHandle) -> Result<(), String> {
    for label in game_labels() {
        if let Some(wv) = app.get_webview(&label) {
            // Recarga nativa (equivale ao F5), sem executar JS na página do jogo
            wv.reload().map_err(|e| format!("{}: {}", label, e))?;
        }
    }
    Ok(())
}

#[tauri::command]
fn get_accounts(app: AppHandle) -> Result<Vec<Account>, String> {
    dpapi::load_accounts(accounts_path(&app)?)
}

#[tauri::command]
fn save_accounts(accounts: Vec<Account>, app: AppHandle) -> Result<(), String> {
    let enc_path = accounts_path(&app)?;
    if let Some(dir) = enc_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Erro ao criar pasta de dados: {}", e))?;
    }
    dpapi::save_accounts(enc_path, &accounts)
}

#[tauri::command]
fn get_twitch_channels() -> Vec<String> {
    TWITCH_CHANNELS.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Salva a lista (normalizada) e repassa às lives, que reavaliam na hora quais canais assistir
#[tauri::command]
fn save_twitch_channels(channels: Vec<String>, app: AppHandle) -> Result<Vec<String>, String> {
    let channels = webviews::channels::normalize(&channels);
    webviews::channels::save(&channels_path(&app)?, &channels)?;
    *TWITCH_CHANNELS.lock().map_err(|e| e.to_string())? = channels.clone();

    let script = twitch_channels_script();
    for label in twitch_labels() {
        if let Some(wv) = app.get_webview(&label) {
            let _ = wv.eval(&script);
        }
    }
    Ok(channels)
}

/// RAM total (bytes) do app e de todos os processos do WebView2, para exibir na barra superior
#[tauri::command]
fn get_memory_usage() -> Result<u64, String> {
    metrics::app_memory_bytes()
}

#[tauri::command]
fn calculate_iv(base: u32, target: u32, level: u32, quality: f64, exp: f64) -> Vec<u32> {
    iv_math::solve_growth_candidates(base, target, level, quality, exp)
}

pub fn create_multi_webviews(window: &WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    let size = window.inner_size()?;
    let scale_factor = window.scale_factor()?;
    let logical = size.to_logical::<f64>(scale_factor);

    let quadrants = webviews::manager::calculate_quadrants(logical.width, logical.height);

    let game_url = WebviewUrl::External(GAME_URL.parse()?);

    let win = window.as_ref().window();
    let app_data = win.app_handle().path().app_data_dir()?;

    // Instancia os 4 quadrantes com pastas de dados independentes no WebView2
    for (i, label) in game_labels().enumerate() {
        let bounds = quadrants[i];
        let profile_dir = app_data.join("profiles").join(&label);

        // Sem initialization_script/eval: a página do jogo roda exatamente como no navegador
        let builder = WebviewBuilder::new(&label, game_url.clone())
            .additional_browser_args(BROWSER_ARGS)
            .data_directory(profile_dir)
            // Sem handler o wry bloqueia todo `window.open`; o login com Google abre um popup que
            // devolve o token via `window.opener`, então o popup padrão do WebView2 precisa ser permitido
            .on_new_window(|_url, _features| NewWindowResponse::Allow)
            // Painel do Modo Eco: religa a leitura passiva do WebSocket depois de cada carga (nada roda na página)
            .on_page_load(|webview, payload| {
                if let PageLoadEvent::Finished = payload.event() {
                    farm::reattach(&webview);
                }
            });

        let webview = win.add_child(builder, bounds.position, bounds.size)?;
        farm::attach(&webview, i + 1);
    }

    // Lives da Twitch e Discord compartilham UMA pasta de dados (mesmas flags = mesmo processo de navegador,
    // GPU e rede) e, com isso, o mesmo login da Twitch. A pasta se chama "twitch" para manter o login existente.
    let aux_profile_dir = app_data.join("profiles").join("twitch");
    let aux_args = format!("{} {} --remote-debugging-port=0", BROWSER_ARGS, TWITCH_AD_HOST_RULES); // TEMP-DEBUG
    let ghost = webviews::manager::calculate_drawer_bounds(logical.width, logical.height, false);

    if let Ok(mut channels) = TWITCH_CHANNELS.lock() {
        *channels = webviews::channels::load(&channels_path(win.app_handle())?);
    }

    let twitch_url = WebviewUrl::External(TWITCH_IDLE_URL.parse()?);
    for (i, label) in twitch_labels().enumerate() {
        let slot = i + 1;
        let twitch_script = format!(
            "window.__PG_TWITCH_SLOT = {};\nwindow.__PG_TWITCH_LIVES = {};\nwindow.__PG_TWITCH_IDLE = {};\n{}\n{}",
            slot,
            TWITCH_LIVES,
            serde_json::Value::from(TWITCH_IDLE_URL),
            twitch_channels_script(),
            webviews::scripts::TWITCH_GHOST_SCRIPT
        );

        let builder = WebviewBuilder::new(&label, twitch_url.clone())
            .initialization_script(&twitch_script)
            .additional_browser_args(&aux_args)
            .data_directory(aux_profile_dir.clone())
            .on_page_load(move |webview, payload| {
                // Mantém o estado real do drawer e a lista atual mesmo após reloads/navegações
                // (só a live 1 vai ao drawer)
                if let PageLoadEvent::Finished = payload.event() {
                    let open = slot == 1 && DRAWER.load(Ordering::Relaxed) == DRAWER_TWITCH;
                    let _ = webview.eval(format!(
                        "{}\n{}",
                        twitch_channels_script(),
                        twitch_ghost_script(open)
                    ));
                }
            });

        let twitch_ghost = webviews::manager::calculate_twitch_ghost_bounds();
        win.add_child(builder, twitch_ghost.position, twitch_ghost.size)?;
    }

    // Discord Web para ficar em call: sem script, só exibido no drawer ou em Ghost (1x1).
    // O microfone usa o pedido de permissão padrão do WebView2 (aceite uma vez com o drawer aberto).
    let discord_builder = WebviewBuilder::new("discord", WebviewUrl::External(DISCORD_URL.parse()?))
        .additional_browser_args(&aux_args)
        .data_directory(aux_profile_dir);
    win.add_child(discord_builder, ghost.position, ghost.size)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            toggle_eco_mode,
            set_drawer,
            set_focus,
            set_overlay_open,
            reload_all_accounts,
            get_accounts,
            save_accounts,
            get_memory_usage,
            get_twitch_channels,
            save_twitch_channels,
            calculate_iv,
            tools::open_tools,
            tools::tools_catalog,
            tools::tools_analyze,
            tools::tools_tierlist,
            tools::tools_hunts_by_type,
            tools::tools_hunts_for_pokemon,
            farm::farm_snapshot,
            farm::farm_reset,
            farm::farm_export
        ])
        .setup(|app| {
            let main_window = app
                .get_webview_window("main")
                .ok_or("Janela principal 'main' não encontrada")?;
            // Histórico do painel do Modo Eco carregado antes de as contas abrirem o WebSocket
            farm::start(app.handle());
            create_multi_webviews(&main_window)?;

            let window = main_window.as_ref().window();
            let win_clone = window.clone();
            let app_handle = app.handle().clone();

            window.on_window_event(move |event| {
                let logical = match event {
                    tauri::WindowEvent::Resized(size) => {
                        size.to_logical::<f64>(win_clone.scale_factor().unwrap_or(1.0))
                    }
                    tauri::WindowEvent::ScaleFactorChanged {
                        scale_factor,
                        new_inner_size,
                        ..
                    } => new_inner_size.to_logical::<f64>(*scale_factor),
                    _ => return,
                };
                layout_webviews(&app_handle, logical.width, logical.height);
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Erro ao executar PokeGrid Ultra-Light")
        .run(|app, event| {
            // Grava o histórico do painel do Modo Eco ao fechar (a gravação periódica é a cada minuto)
            if let tauri::RunEvent::Exit = event {
                farm::save(app);
            }
        });
}
