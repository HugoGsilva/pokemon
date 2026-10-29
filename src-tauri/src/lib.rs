pub mod domain;
pub mod security;
pub mod webviews;

use domain::iv_math;
use security::dpapi::{self, Account};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    AppHandle, Manager, Runtime, Webview, WebviewUrl, WebviewWindow,
};

static TWITCH_DRAWER_OPEN: AtomicBool = AtomicBool::new(false);
static ECO_ENABLED: AtomicBool = AtomicBool::new(false);

// Regras do jogo (https://poke.idleworld.online/rules):
// - mais de 4 contas por jogador leva a exclusão permanente;
// - scripts, extensões, macros e automações sem permissão da staff são proibidos.
// Por isso as webviews do jogo NÃO recebem nenhum script injetado: o app só exibe a interface oficial.
const MAX_ACCOUNTS: usize = 4;
const GAME_URL: &str = "https://poke.idleworld.online";
// Diretório da categoria: o script da Twitch entra sozinho na primeira live listada
const TWITCH_URL: &str = "https://www.twitch.tv/directory/category/pokemon-community-game";

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

fn twitch_ghost_script(drawer_open: bool) -> String {
    format!(
        "if (window.applyTwitchGhostMode) window.applyTwitchGhostMode({});",
        !drawer_open
    )
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

    let quadrants = webviews::manager::calculate_quadrants(width, height);
    for (i, label) in game_labels().enumerate() {
        if let Some(wv) = app.get_webview(&label) {
            let b = quadrants[i];
            let _ = wv.set_position(b.position);
            let _ = wv.set_size(b.size);
        }
    }

    if let Some(tw_wv) = app.get_webview("twitch") {
        let is_open = TWITCH_DRAWER_OPEN.load(Ordering::Relaxed);
        let tb = webviews::manager::calculate_twitch_bounds(width, height, is_open);
        let _ = tw_wv.set_position(tb.position);
        let _ = tw_wv.set_size(tb.size);
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

#[tauri::command]
fn toggle_twitch_drawer(open: bool, app: AppHandle) -> Result<(), String> {
    TWITCH_DRAWER_OPEN.store(open, Ordering::Relaxed);

    if let Some((width, height)) = main_logical_size(&app) {
        layout_webviews(&app, width, height);
    }
    if let Some(tw_wv) = app.get_webview("twitch") {
        tw_wv
            .eval(twitch_ghost_script(open))
            .map_err(|e| e.to_string())?;
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

    // Em Ghost (1x1) a Twitch não cobre o modal; escondê-la pausaria o vídeo dos drops
    if TWITCH_DRAWER_OPEN.load(Ordering::Relaxed) {
        if let Some(tw_wv) = app.get_webview("twitch") {
            let result = if open { tw_wv.hide() } else { tw_wv.show() };
            result.map_err(|e| e.to_string())?;
        }
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
            .on_new_window(|_url, _features| NewWindowResponse::Allow);

        win.add_child(builder, bounds.position, bounds.size)?;
    }

    // Instancia a 5ª Webview para Twitch (Drops + Ghost Mode)
    let twitch_url = WebviewUrl::External(TWITCH_URL.parse()?);
    let twitch_bounds = webviews::manager::calculate_twitch_bounds(
        logical.width,
        logical.height,
        TWITCH_DRAWER_OPEN.load(Ordering::Relaxed),
    );
    let twitch_script = format!(
        "window.__PG_TWITCH_DIRECTORY = {};\n{}",
        serde_json::Value::from(TWITCH_URL),
        webviews::scripts::TWITCH_GHOST_SCRIPT
    );

    let twitch_profile_dir = app_data.join("profiles").join("twitch");
    let twitch_builder = WebviewBuilder::new("twitch", twitch_url)
        .initialization_script(&twitch_script)
        .additional_browser_args(&format!("{} {}", BROWSER_ARGS, TWITCH_AD_HOST_RULES))
        .data_directory(twitch_profile_dir)
        .on_page_load(|webview, payload| {
            // Mantém o estado real do drawer mesmo após reloads/navegações
            if let PageLoadEvent::Finished = payload.event() {
                let _ = webview.eval(twitch_ghost_script(
                    TWITCH_DRAWER_OPEN.load(Ordering::Relaxed),
                ));
            }
        });

    win.add_child(twitch_builder, twitch_bounds.position, twitch_bounds.size)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            toggle_eco_mode,
            toggle_twitch_drawer,
            set_overlay_open,
            reload_all_accounts,
            get_accounts,
            save_accounts,
            calculate_iv
        ])
        .setup(|app| {
            let main_window = app
                .get_webview_window("main")
                .ok_or("Janela principal 'main' não encontrada")?;
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
        .run(tauri::generate_context!())
        .expect("Erro ao executar PokeGrid Ultra-Light");
}
