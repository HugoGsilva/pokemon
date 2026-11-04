//! Janela "Ferramentas" (Qualidade/IV, Tierlist e Onde caçar). Os dados vêm dos arquivos públicos do site do
//! jogo, baixados aqui no Rust: nada roda dentro das webviews das contas.
use crate::domain::catalog::{Catalog, Species};
use crate::domain::hunt_model::{self, Attacker, HuntRow, TierRow, Tms, TypeMatchups};
use crate::domain::iv_math::{self, IvInput, IvResult, Projection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};

const GAME_ORIGIN: &str = "https://poke.idleworld.online";
const SOURCES: [&str; 3] = ["/game/creatures.json", "/game/items.json", "/api/game/map-markers"];
/// O jogo atualiza o catálogo em patches; um dia de cache evita baixar ~1,8 MB a cada abertura
const MAX_AGE_S: u64 = 24 * 3600;
const RETRY_S: u64 = 10 * 60;

static CATALOG: Mutex<Option<Arc<Catalog>>> = Mutex::new(None);
static LAST_TRY: AtomicU64 = AtomicU64::new(0);
/// Última tierlist calculada (nível, TM, gold): trocar de aba de elemento não recalcula
static TIER_CACHE: Mutex<Option<((u64, bool, bool, u64), Arc<Vec<TierRow>>)>> = Mutex::new(None);

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cache_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Erro ao obter diretório de dados: {}", e))?;
    Ok(dir.join("gamedata.json"))
}

fn download() -> Result<Catalog, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(30))
        .user_agent("PokeGrid-UltraLight")
        .build();
    let mut bodies = Vec::with_capacity(SOURCES.len());
    for path in SOURCES {
        let body = agent
            .get(&format!("{}{}", GAME_ORIGIN, path))
            .call()
            .map_err(|e| format!("{}: {}", path, e))?
            .into_string()
            .map_err(|e| format!("{}: {}", path, e))?;
        bodies.push(body);
    }
    Catalog::build(&bodies[0], &bodies[1], &bodies[2], now_s())
}

fn read_cache(path: &PathBuf) -> Option<Catalog> {
    let mut cat: Catalog = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    cat.reindex();
    Some(cat)
}

/// Catálogo em memória; baixa de novo se o cache tiver mais de um dia (ou `force`). Sem internet usa o cache velho
/// e só tenta baixar outra vez depois de `RETRY_S`, para não travar cada comando no timeout
pub(crate) fn catalog<R: Runtime>(app: &AppHandle<R>, force: bool) -> Result<(Arc<Catalog>, Option<String>), String> {
    let mut slot = CATALOG.lock().map_err(|e| e.to_string())?;
    let path = cache_path(app)?;
    if slot.is_none() {
        *slot = read_cache(&path).map(Arc::new);
    }
    let now = now_s();
    let stale = slot.as_ref().map(|c| now.saturating_sub(c.fetched_at) >= MAX_AGE_S);
    let tried_recently = now.saturating_sub(LAST_TRY.load(Ordering::Relaxed)) < RETRY_S;
    if let (Some(cat), Some(stale)) = (slot.as_ref(), stale) {
        if !force && (!stale || tried_recently) {
            let warning = stale.then(|| "Sem conexão com o jogo; usando os dados salvos".to_string());
            return Ok((cat.clone(), warning));
        }
    }

    LAST_TRY.store(now, Ordering::Relaxed);
    match download() {
        Ok(cat) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_vec(&cat) {
                let _ = std::fs::write(&path, json);
            }
            let cat = Arc::new(cat);
            *slot = Some(cat.clone());
            Ok((cat, None))
        }
        Err(err) => match slot.as_ref() {
            Some(cat) => Ok((cat.clone(), Some(format!("Sem conexão com o jogo; usando os dados salvos ({})", err)))),
            None => Err(format!("Não foi possível baixar os dados do jogo: {}", err)),
        },
    }
}

/// Catálogo já em memória, sem baixar nem esperar um download em andamento (para o painel do Modo Eco)
pub(crate) fn catalog_loaded() -> Option<Arc<Catalog>> {
    CATALOG.try_lock().ok()?.clone()
}

fn species<'a>(cat: &'a Catalog, name: &str) -> Result<&'a Species, String> {
    cat.species_by_name(name).ok_or_else(|| format!("Espécie não encontrada: {}", name))
}

/// Abre (ou traz para frente) a janela de ferramentas, declarada em tauri.conf.json com `create: false`.
/// Precisa ser `async`: comando síncrono roda na thread principal, e criar uma janela de lá trava o app no Windows
/// (o WebView2 espera o loop de eventos, que está preso esperando o comando)
#[tauri::command(async)]
pub fn open_tools(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("tools") {
        let _ = win.unminimize();
        return win.set_focus().map_err(|e| e.to_string());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "tools")
        .ok_or("Janela 'tools' não declarada no tauri.conf.json")?
        .clone();
    WebviewWindowBuilder::from_config(&app, &config)
        .and_then(|b| b.build())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeciesBrief {
    name: String,
    id: u32,
    types: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInfo {
    fetched_at: u64,
    hunts: usize,
    species: Vec<SpeciesBrief>,
    warning: Option<String>,
}

#[tauri::command(async)]
pub fn tools_catalog(force: bool, app: AppHandle) -> Result<CatalogInfo, String> {
    let (cat, warning) = catalog(&app, force)?;
    Ok(catalog_info(&cat, warning))
}

fn catalog_info(cat: &Catalog, warning: Option<String>) -> CatalogInfo {
    let mut species: Vec<SpeciesBrief> = cat
        .species
        .iter()
        .map(|s| SpeciesBrief { name: s.name.clone(), id: s.id, types: s.types.clone() })
        .collect();
    species.sort_by(|a, b| a.name.cmp(&b.name));
    CatalogInfo { fetched_at: cat.fetched_at, hunts: cat.hunts.len(), species, warning }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeInput {
    species: String,
    level: f64,
    quality: f64,
    quality_decimals: u32,
    iv_total: Option<f64>,
    power: Option<f64>,
    stats: [Option<f64>; 6],
    target_level: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    species: Species,
    iv: IvResult,
    current: Option<Projection>,
    target: Option<Projection>,
    /// Atributo de cada stat com IV 32 no nível do card (quanto falta para o máximo)
    max_stats: Option<[u32; 6]>,
    matchups: TypeMatchups,
}

#[tauri::command(async)]
pub fn tools_analyze(input: AnalyzeInput, app: AppHandle) -> Result<Analysis, String> {
    let (cat, _) = catalog(&app, false)?;
    analyze(&cat, input)
}

fn analyze(cat: &Catalog, input: AnalyzeInput) -> Result<Analysis, String> {
    let sp = species(cat, &input.species)?.clone();
    let has_stats = input.stats.iter().all(|v| v.is_some());
    let iv = iv_math::normalize_ivs(&IvInput {
        bases: sp.bases,
        level: input.level,
        quality: input.quality,
        quality_decimals: input.quality_decimals,
        iv_total: input.iv_total,
        power: input.power,
        observed: has_stats.then_some(input.stats),
        ivs: None,
    });
    let current = iv_math::project(&sp.bases, &iv, input.quality, input.level);
    let target = input
        .target_level
        .filter(|l| *l > 0.0)
        .and_then(|l| iv_math::project(&sp.bases, &iv, input.quality, l));
    let max_iv = IvResult { values: [iv_math::IV_MAX; 6], ..iv.clone() };
    let max_stats = iv_math::project(&sp.bases, &max_iv, input.quality, input.level).map(|p| p.stats);
    let matchups = hunt_model::matchups(&sp.types);
    Ok(Analysis { species: sp, iv, current, target, max_stats, matchups })
}

#[tauri::command(async)]
pub fn tools_tierlist(level: f64, with_tm: bool, gold: bool, app: AppHandle) -> Result<Arc<Vec<TierRow>>, String> {
    let (cat, _) = catalog(&app, false)?;
    let key = (level.max(0.0) as u64, with_tm, gold, cat.fetched_at);
    if let Some((k, rows)) = TIER_CACHE.lock().map_err(|e| e.to_string())?.as_ref() {
        if *k == key {
            return Ok(rows.clone());
        }
    }
    let rows = Arc::new(hunt_model::tier_list(&cat, level.max(0.0), with_tm, gold));
    *TIER_CACHE.lock().map_err(|e| e.to_string())? = Some((key, rows.clone()));
    Ok(rows)
}

#[tauri::command(async)]
pub fn tools_hunts_by_type(element: String, level: f64, with_tm: bool, gold: bool, app: AppHandle) -> Result<Vec<HuntRow>, String> {
    let (cat, _) = catalog(&app, false)?;
    Ok(hunt_model::hunts_by_type(&cat, &element.to_uppercase(), level.max(1.0), with_tm, gold))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PokemonHuntsInput {
    species: String,
    level: f64,
    quality: f64,
    iv_total: Option<f64>,
    with_tm: bool,
    gold: bool,
    /// Nível da conta: só hunts que ela alcança (0 = todas)
    max_hunt_level: f64,
}

#[tauri::command(async)]
pub fn tools_hunts_for_pokemon(input: PokemonHuntsInput, app: AppHandle) -> Result<Vec<HuntRow>, String> {
    let (cat, _) = catalog(&app, false)?;
    pokemon_hunts(&cat, input)
}

fn pokemon_hunts(cat: &Catalog, input: PokemonHuntsInput) -> Result<Vec<HuntRow>, String> {
    let sp = species(cat, &input.species)?;
    let attacker = Attacker {
        species: sp,
        level: input.level.max(1.0),
        quality: if input.quality > 0.0 { input.quality } else { 1.0 },
        iv: iv_math::scenario_from_total(Some(input.iv_total.unwrap_or(iv_math::IV_TOTAL_MEDIO))),
        tms: if input.with_tm { Tms::All } else { Tms::None },
    };
    Ok(hunt_model::hunts_for_pokemon(cat, &attacker, input.max_hunt_level, input.gold))
}

#[cfg(test)]
mod tests {
    /// Baixa os dados de verdade do site do jogo: `cargo test --lib baixa_dados -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn baixa_dados_do_jogo() {
        let cat = super::download().unwrap();
        println!("{} espécies, {} hunts", cat.species.len(), cat.hunts.len());
        assert!(cat.species.len() > 400 && cat.hunts.len() > 300);
        assert!(cat.species_by_name("Gyarados").is_some());
    }

    /// Respostas reais dos comandos, para testar a interface no navegador com o IPC simulado:
    /// `PG_FIXTURES=<pasta> cargo test --release --lib gera_fixtures -- --ignored`
    #[test]
    #[ignore]
    fn gera_fixtures_da_interface() {
        use super::*;
        let out = PathBuf::from(std::env::var("PG_FIXTURES").expect("PG_FIXTURES"));
        std::fs::create_dir_all(&out).unwrap();
        let write = |name: &str, v: serde_json::Value| std::fs::write(out.join(format!("{name}.json")), v.to_string()).unwrap();
        let cat = download().unwrap();
        write("tools_catalog", serde_json::to_value(catalog_info(&cat, None)).unwrap());
        // Tyranitar real (qualidade 1,71 na tela, 1,7062 de verdade)
        let input: AnalyzeInput = serde_json::from_value(serde_json::json!({
            "species": "Tyranitar", "level": 149, "quality": 1.71, "qualityDecimals": 2, "ivTotal": 140, "power": 3513,
            "stats": [386, 452, 370, 240, 361, 250], "targetLevel": 500
        }))
        .unwrap();
        write("tools_analyze", serde_json::to_value(analyze(&cat, input).unwrap()).unwrap());
        write("tools_tierlist", serde_json::to_value(hunt_model::tier_list(&cat, 100.0, false, false)).unwrap());
        write("tools_hunts_by_type", serde_json::to_value(hunt_model::hunts_by_type(&cat, "FIRE", 100.0, false, false)).unwrap());
        let input: PokemonHuntsInput = serde_json::from_value(serde_json::json!({
            "species": "Tyranitar", "level": 149, "quality": 1.71, "ivTotal": 140, "withTm": false, "gold": false, "maxHuntLevel": 150
        }))
        .unwrap();
        write("tools_hunts_for_pokemon", serde_json::to_value(pokemon_hunts(&cat, input).unwrap()).unwrap());
    }
}
