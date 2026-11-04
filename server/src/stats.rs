//! Painel do Modo Eco no servidor: a mesma contabilidade do app (`domain::farm`), com o histórico em
//! `PG_DATA/farm_history.json` e os preços de NPC do catálogo público do jogo.
use crate::catalog::Catalog;
use crate::farm::{pretty_slug, FarmState, History, Prices};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SOURCES: [&str; 3] = ["/game/creatures.json", "/game/items.json", "/api/game/map-markers"];
const CATALOG_MAX_AGE_S: u64 = 24 * 3600;
const CATALOG_RETRY: Duration = Duration::from_secs(10 * 60);
const SAVE_EVERY: Duration = Duration::from_secs(60);

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub struct Stats {
    pub farm: Mutex<FarmState>,
    catalog: Mutex<Option<Arc<Catalog>>>,
    history_path: PathBuf,
    catalog_path: PathBuf,
    game_url: String,
}

impl Stats {
    pub fn load(data_dir: &std::path::Path, game_url: &str) -> Arc<Stats> {
        let history_path = data_dir.join("farm_history.json");
        let history = std::fs::read(&history_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<History>(&b).ok())
            .unwrap_or_default();
        let catalog_path = data_dir.join("gamedata.json");
        let catalog = std::fs::read(&catalog_path).ok().and_then(|b| serde_json::from_slice::<Catalog>(&b).ok()).map(
            |mut c| {
                c.reindex();
                Arc::new(c)
            },
        );
        let mut farm = FarmState::new(history, now_ms());
        if let Some(c) = &catalog {
            farm.set_prices(Prices { by_id: c.item_prices.clone() });
        }
        Arc::new(Stats {
            farm: Mutex::new(farm),
            catalog: Mutex::new(catalog),
            history_path,
            catalog_path,
            game_url: game_url.trim_end_matches('/').to_string(),
        })
    }

    /// Nome da hunt pelo catálogo; sem catálogo, o slug com espaços
    pub fn hunt_name(&self, slug: &str) -> String {
        if slug.is_empty() {
            return String::new();
        }
        self.catalog
            .lock()
            .ok()
            .and_then(|c| c.as_ref()?.hunts.iter().find(|h| h.slug == slug).map(|h| h.name.clone()))
            .unwrap_or_else(|| pretty_slug(slug))
    }

    /// Grava o histórico se mudou (arquivo temporário + rename, para não corromper num desligamento)
    pub fn save(&self) {
        let json = {
            let Ok(mut st) = self.farm.lock() else { return };
            if !st.dirty {
                return;
            }
            st.history.prune(now_ms());
            st.dirty = false;
            serde_json::to_vec(&st.history)
        };
        let Ok(json) = json else { return };
        let tmp = self.history_path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &self.history_path);
        }
    }

    fn download(&self) -> Result<Catalog, String> {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent("PokeGrid-Remoto").build();
        let mut bodies = Vec::with_capacity(SOURCES.len());
        for path in SOURCES {
            let body = agent
                .get(&format!("{}{}", self.game_url, path))
                .call()
                .map_err(|e| e.to_string())
                .and_then(|r| r.into_string().map_err(|e| e.to_string()))
                .map_err(|e| format!("{}: {}", path, e))?;
            bodies.push(body);
        }
        let now_s = now_ms() / 1000;
        Catalog::build(&bodies[0], &bodies[1], &bodies[2], now_s)
    }

    /// Baixa o catálogo se não houver um de menos de um dia. Devolve se ainda falta tentar de novo logo
    fn refresh_catalog(&self) -> bool {
        let fresh = self
            .catalog
            .lock()
            .ok()
            .and_then(|c| c.as_ref().map(|c| (now_ms() / 1000).saturating_sub(c.fetched_at) < CATALOG_MAX_AGE_S))
            .unwrap_or(false);
        if fresh {
            return false;
        }
        match self.download() {
            Ok(cat) => {
                if let Ok(json) = serde_json::to_vec(&cat) {
                    let _ = std::fs::write(&self.catalog_path, json);
                }
                if let Ok(mut farm) = self.farm.lock() {
                    farm.set_prices(Prices { by_id: cat.item_prices.clone() });
                }
                if let Ok(mut slot) = self.catalog.lock() {
                    *slot = Some(Arc::new(cat));
                }
                false
            }
            Err(err) => {
                eprintln!("[stats] catálogo do jogo: {}", err);
                true
            }
        }
    }

    /// Grava o histórico a cada minuto e mantém o catálogo em dia
    pub fn spawn_background(self: &Arc<Self>) {
        let stats = self.clone();
        tokio::spawn(async move {
            let mut last_try: Option<std::time::Instant> = None;
            let mut retry = false;
            loop {
                let due = match last_try {
                    None => true,
                    Some(t) => !retry || t.elapsed() >= CATALOG_RETRY,
                };
                if due {
                    last_try = Some(std::time::Instant::now());
                    let s = stats.clone();
                    retry = tokio::task::spawn_blocking(move || s.refresh_catalog()).await.unwrap_or(true);
                }
                tokio::time::sleep(SAVE_EVERY).await;
                let s = stats.clone();
                let _ = tokio::task::spawn_blocking(move || s.save()).await;
            }
        });
    }
}
