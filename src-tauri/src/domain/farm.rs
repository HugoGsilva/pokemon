//! Painel do Modo Eco: soma o que cada conta farma a partir das mensagens que o servidor do jogo manda pelo
//! WebSocket da página (kills, XP, loot, capturas, shinies). As mensagens chegam já lidas (no app,
//! `crate::farm`; no servidor remoto, `server/src/account.rs`); aqui só existe a contabilidade, sem nada de
//! webview, para poder testar.
//!
//! Mensagens usadas (as mesmas que o PokeGrid original lê, MIT):
//! - `field-init {slug}`: entrou numa hunt; `field-none` / `field-teleport-city`: saiu
//! - `field-kill {xpGained, loot:[{itemId,name,qty}], shiny, speciesName, speciesId}`: um abate
//! - `catch-result {ballId, success}`: uma pokébola gasta
//! - `pokes {list}` / `pokes-chunk {gen,seq,total,list}` / `poke-delta {poke}`: a caixa; id novo depois da
//!   primeira lista é captura
//! - `poke-xp {id, level}`: nível de um pokémon do time
//! - `balls {catalog, counts}`: estoque e preço das pokébolas
//! - `field {fainted}`: time inteiro derrotado
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

pub const ACCOUNTS: usize = 4;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
/// Histórico por hora guardado (o painel mostra até 90 dias)
const KEEP_MS: u64 = 92 * DAY_MS;
/// Drops por item só ficam nas horas recentes (o painel mostra os de hoje; a exportação usa as hunts)
const KEEP_DROPS_MS: u64 = 8 * DAY_MS;
const MAX_CATCHES: usize = 500;
const MAX_SHINIES: usize = 300;
const MAX_HUNTS: usize = 500;
/// Shiny derrotado e capturado logo depois é o mesmo encontro, não dois
const SHINY_MATCH_MS: u64 = 60_000;
/// Sem kill por esse tempo dentro da hunt: conta parada
const STALLED_MS: u64 = 10 * 60_000;
/// Socket fechado por mais que isso: desconectada (o jogo reconecta sozinho em quedas curtas)
const OFFLINE_MS: u64 = 20_000;
/// Pokébolas que acabam antes disso viram aviso no status
const BALLS_WARN_MIN: f64 = 4.0 * 60.0;
/// Partes de uma lista `pokes-chunk` além disso são descartadas (lista malformada)
const MAX_CHUNKS: usize = 512;

/// Preço de NPC de cada item (o mesmo que o Hunt Analyzer do jogo usa)
#[derive(Debug, Default, Clone)]
pub struct Prices {
    pub by_id: HashMap<u32, f64>,
}

impl Prices {
    fn of(&self, id: u32) -> f64 {
        self.by_id.get(&id).copied().unwrap_or(0.0)
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Totals {
    /// Estimativa: loot a preço de NPC + venda das capturas − pokébolas gastas
    pub gold: f64,
    pub xp: f64,
    pub kills: u64,
    pub caps: u64,
    pub shiny_seen: u64,
    pub shiny_caught: u64,
}

impl Totals {
    fn add(&mut self, o: &Totals) {
        self.gold += o.gold;
        self.xp += o.xp;
        self.kills += o.kills;
        self.caps += o.caps;
        self.shiny_seen += o.shiny_seen;
        self.shiny_caught += o.shiny_caught;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DropRow {
    /// Conta (1..=4)
    pub a: u8,
    pub id: u32,
    pub name: String,
    pub qty: f64,
    pub gold: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HourBucket {
    #[serde(flatten)]
    pub totals: Totals,
    pub drops: Vec<DropRow>,
}

fn add_drop(rows: &mut Vec<DropRow>, a: u8, id: u32, name: &str, qty: f64, gold: f64) {
    match rows.iter_mut().find(|r| r.a == a && r.id == id) {
        Some(r) => {
            r.qty += qty;
            r.gold += gold;
        }
        None => rows.push(DropRow { a, id, name: name.to_string(), qty, gold }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatchEntry {
    pub account: u8,
    pub name: String,
    pub species_id: u32,
    pub iv: f64,
    pub q: f64,
    pub shiny: bool,
    /// Primeiro da espécie na caixa da conta
    pub new: bool,
    /// Pokébolas gastas até esta captura (0 = desconhecido)
    pub balls: u32,
    pub t: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShinyEntry {
    pub account: u8,
    pub name: String,
    pub species_id: u32,
    pub t: u64,
    pub captured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HuntDrop {
    pub name: String,
    pub qty: f64,
    pub gold: f64,
}

/// Uma passagem por uma hunt, gravada quando a conta troca de hunt (ou no Zerar)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HuntRecord {
    pub account: u8,
    pub slug: String,
    pub start: u64,
    pub end: u64,
    pub kills: u64,
    pub xp: f64,
    pub caps: u64,
    pub gold: f64,
    pub shiny_seen: u64,
    pub balls: u64,
    pub drops: Vec<HuntDrop>,
}

/// O que fica salvo em disco entre aberturas do app
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct History {
    /// Início da hora (Unix ms, UTC) -> somas daquela hora
    pub hours: BTreeMap<u64, HourBucket>,
    pub catches: VecDeque<CatchEntry>,
    pub shinies: VecDeque<ShinyEntry>,
    pub hunts: VecDeque<HuntRecord>,
}

impl History {
    fn bucket(&mut self, now: u64) -> &mut HourBucket {
        self.hours.entry(now - now % HOUR_MS).or_default()
    }

    /// Descarta horas velhas e os drops por item fora da janela recente
    pub fn prune(&mut self, now: u64) {
        let cut = now.saturating_sub(KEEP_MS);
        self.hours.retain(|&h, _| h >= cut);
        let cut_drops = now.saturating_sub(KEEP_DROPS_MS);
        for (_, b) in self.hours.range_mut(..cut_drops) {
            b.drops.clear();
        }
        while self.catches.len() > MAX_CATCHES {
            self.catches.pop_front();
        }
        while self.shinies.len() > MAX_SHINIES {
            self.shinies.pop_front();
        }
        while self.hunts.len() > MAX_HUNTS {
            self.hunts.pop_front();
        }
    }

    fn sum_since(&self, since: u64) -> Totals {
        let mut t = Totals::default();
        for (_, b) in self.hours.range(since..) {
            t.add(&b.totals);
        }
        t
    }

    /// Somas por dia local dos últimos `days` dias, do mais novo para o mais velho
    pub fn days(&self, now: u64, tz_offset_min: i32, days: u64) -> Vec<(u64, Totals)> {
        let today = local_day_start(now, tz_offset_min);
        (0..days)
            .filter_map(|i| {
                let start = today.checked_sub(i * DAY_MS)?;
                let mut t = Totals::default();
                for (_, b) in self.hours.range(start..start + DAY_MS) {
                    t.add(&b.totals);
                }
                Some((start, t))
            })
            .collect()
    }
}

/// Unix ms (UTC) da meia-noite local de hoje. `tz_offset_min` é o `Date.getTimezoneOffset()` do JS
/// (UTC − local, em minutos: 180 no horário de Brasília)
pub fn local_day_start(now: u64, tz_offset_min: i32) -> u64 {
    let offset = tz_offset_min as i64 * 60_000;
    let local = now as i64 - offset;
    let day = local.div_euclid(DAY_MS as i64) * DAY_MS as i64;
    (day + offset).max(0) as u64
}

/// Data e hora locais (`AAAA-MM-DD HH:MM`) de um instante Unix ms
pub fn local_datetime(t: u64, tz_offset_min: i32) -> String {
    let local = t as i64 - tz_offset_min as i64 * 60_000;
    let days = local.div_euclid(DAY_MS as i64);
    let ms = local.rem_euclid(DAY_MS as i64);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, ms / HOUR_MS as i64, ms % HOUR_MS as i64 / 60_000)
}

/// Dias desde 1970-01-01 -> (ano, mês, dia), algoritmo de Howard Hinnant
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[derive(Debug, Default, Clone)]
struct Session {
    slug: String,
    start: u64,
    last_kill: u64,
    kills: u64,
    xp: f64,
    caps: u64,
    balls: u64,
    gold: f64,
    shiny_seen: u64,
    shiny_caught: u64,
    /// itemId -> (nome, quantidade, gold)
    drops: HashMap<u32, (String, f64, f64)>,
}

impl Session {
    fn new(slug: &str, now: u64) -> Self {
        Session { slug: slug.to_string(), start: now, ..Default::default() }
    }

    /// Hunt que valeu a pena guardar (teve abate)
    fn record(&self, account: u8, now: u64) -> Option<HuntRecord> {
        if self.kills == 0 {
            return None;
        }
        let mut drops: Vec<HuntDrop> = self
            .drops
            .values()
            .map(|(name, qty, gold)| HuntDrop { name: name.clone(), qty: *qty, gold: *gold })
            .collect();
        drops.sort_by(|a, b| b.gold.total_cmp(&a.gold).then(b.qty.total_cmp(&a.qty)));
        Some(HuntRecord {
            account,
            slug: self.slug.clone(),
            start: self.start,
            end: if self.last_kill >= self.start { self.last_kill } else { now },
            kills: self.kills,
            xp: self.xp,
            caps: self.caps,
            gold: self.gold,
            shiny_seen: self.shiny_seen,
            balls: self.balls,
            drops,
        })
    }
}

#[derive(Debug, Clone)]
struct TeamMon {
    name: String,
    level: f64,
    leader: bool,
    slot: f64,
}

#[derive(Debug, Default)]
struct PokesChunks {
    gen: i64,
    total: usize,
    parts: BTreeMap<usize, Vec<Value>>,
}

#[derive(Debug, Default)]
struct Account {
    slot: u8,
    /// requestId (DevTools) do WebSocket do jogo
    socket: Option<String>,
    connected: bool,
    ever_connected: bool,
    closed_at: u64,
    last_msg: u64,
    in_hunt: bool,
    hunt_since: u64,
    fainted: bool,
    session: Session,
    known_pokes: HashSet<String>,
    baselined: bool,
    species_owned: HashSet<u32>,
    team: HashMap<String, TeamMon>,
    chunks: Option<PokesChunks>,
    /// None = estoque desconhecido; infinito = pokébola infinita
    balls_left: Option<f64>,
    /// ballId -> (preço em gold, infinita)
    ball_info: HashMap<i64, (f64, bool)>,
    since_catch: u32,
    pending_balls: u32,
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        // O jogo manda a qualidade como texto ("1.2") e às vezes com vírgula
        Value::String(s) => {
            let s = s.trim().replace(',', ".");
            let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(s.len());
            s[..end].parse().unwrap_or(0.0)
        }
        _ => 0.0,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.chars().take(60).collect(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Id de um pokémon da caixa (número ou texto)
fn poke_id(pk: &Value) -> Option<String> {
    match pk.get("id")? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn species_of(pk: &Value) -> u32 {
    let sid = num(pk.get("speciesId").unwrap_or(&Value::Null));
    let sid = if sid > 0.0 { sid } else { num(pk.get("pokeId").unwrap_or(&Value::Null)) };
    sid.max(0.0) as u32
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    }
}

impl Account {
    fn new(slot: u8, now: u64) -> Self {
        Account { slot, session: Session::new("", now), ..Default::default() }
    }

    fn update_team(&mut self, id: &str, pk: &Value) {
        // poke-delta pode trazer só parte dos campos: sem `team` mantém o que se sabia
        let in_team = match pk.get("team") {
            Some(v) => truthy(Some(v)),
            None => self.team.contains_key(id),
        };
        if !in_team {
            self.team.remove(id);
            return;
        }
        let prev = self.team.get(id).cloned();
        let field = |k: &str| pk.get(k).filter(|v| !v.is_null());
        let mon = TeamMon {
            name: field("name").map(text).or_else(|| prev.as_ref().map(|p| p.name.clone())).unwrap_or_default(),
            level: field("level").map(num).or_else(|| prev.as_ref().map(|p| p.level)).unwrap_or(0.0),
            leader: match pk.get("leader") {
                Some(v) => truthy(Some(v)),
                None => prev.as_ref().is_some_and(|p| p.leader),
            },
            slot: field("slot").map(num).or_else(|| prev.as_ref().map(|p| p.slot)).unwrap_or(99.0),
        };
        self.team.insert(id.to_string(), mon);
    }

    fn leader(&self) -> Option<&TeamMon> {
        self.team
            .values()
            .find(|m| m.leader)
            .or_else(|| self.team.values().min_by(|a, b| a.slot.total_cmp(&b.slot)))
    }
}

/// Uma linha do painel por conta
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub slot: u8,
    /// waiting | offline | fainted | stalled | balls | hunting | city
    pub status: &'static str,
    /// Minutos parada (stalled) ou até acabar as pokébolas (balls)
    pub status_min: f64,
    pub pokemon: String,
    pub level: f64,
    pub in_hunt: bool,
    pub hunt_slug: String,
    /// Preenchido pelo app com o catálogo do jogo
    pub hunt_name: String,
    /// Duração da sessão usada nas taxas por hora
    pub elapsed_ms: u64,
    pub gold: f64,
    pub xp: f64,
    pub kills: u64,
    pub caps: u64,
    pub balls: u64,
    pub shiny_seen: u64,
    pub balls_left: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShinyView {
    #[serde(flatten)]
    pub entry: ShinyEntry,
    /// Derrotado há menos de um minuto: ainda pode ser capturado
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub now: u64,
    pub today: Totals,
    pub week: Totals,
    pub quarter: Totals,
    pub accounts: Vec<AccountView>,
    pub catches: Vec<CatchEntry>,
    pub shinies: Vec<ShinyView>,
    /// Desde a abertura do app (ou o Zerar): capturados, apareceram, perdidos
    pub shiny_session: [u64; 3],
    pub drops_today: Vec<DropRow>,
    pub hunts_count: usize,
}

pub struct FarmState {
    accounts: Vec<Account>,
    pub history: History,
    prices: Prices,
    /// Início dos contadores de sessão (abertura do app ou Zerar)
    epoch: u64,
    pub dirty: bool,
}

impl FarmState {
    pub fn new(history: History, now: u64) -> Self {
        FarmState {
            accounts: (1..=ACCOUNTS as u8).map(|s| Account::new(s, now)).collect(),
            history,
            prices: Prices::default(),
            epoch: now,
            dirty: false,
        }
    }

    pub fn set_prices(&mut self, prices: Prices) {
        self.prices = prices;
    }

    pub fn has_prices(&self) -> bool {
        !self.prices.by_id.is_empty()
    }

    /// A conta está conectada e numa hunt (o servidor remoto espera um abate antes de recarregar a página)
    pub fn in_hunt(&self, slot: usize) -> bool {
        slot.checked_sub(1).and_then(|i| self.accounts.get(i)).is_some_and(|a| a.connected && a.in_hunt)
    }

    fn account(&mut self, slot: usize) -> Option<&mut Account> {
        self.accounts.get_mut(slot.checked_sub(1)?)
    }

    /// O WebSocket do jogo (wss://poke.idleworld.online/ws...) abriu nesta conta
    pub fn socket_created(&mut self, slot: usize, request_id: &str, url: &str) {
        if !is_game_socket(url) {
            return;
        }
        if let Some(acc) = self.account(slot) {
            acc.socket = Some(request_id.to_string());
            acc.connected = true;
            acc.ever_connected = true;
            // A lista em partes recomeça a cada conexão, como o jogo faz
            acc.chunks = None;
        }
    }

    pub fn socket_closed(&mut self, slot: usize, request_id: &str, now: u64) {
        if let Some(acc) = self.account(slot) {
            if acc.socket.as_deref() == Some(request_id) {
                acc.connected = false;
                acc.closed_at = now;
            }
        }
    }

    /// Frame de texto recebido por um WebSocket da conta
    pub fn frame(&mut self, slot: usize, request_id: &str, payload: &str, now: u64) {
        let Some(acc) = self.account(slot) else { return };
        match acc.socket.as_deref() {
            Some(id) if id == request_id => {}
            // A abertura do socket passou antes de a leitura começar: adota o primeiro que falar
            None => {
                acc.socket = Some(request_id.to_string());
                acc.connected = true;
                acc.ever_connected = true;
            }
            Some(_) => return,
        }
        // O jogo só manda objetos JSON; o resto (ping etc.) fica de fora sem custo
        if !payload.starts_with('{') {
            return;
        }
        if let Ok(msg) = serde_json::from_str::<Value>(payload) {
            self.handle(slot, &msg, now);
        }
    }

    /// Aplica uma mensagem do jogo já decodificada
    pub fn handle(&mut self, slot: usize, msg: &Value, now: u64) {
        let Some(ty) = msg.get("type").and_then(Value::as_str) else { return };
        let Some(idx) = slot.checked_sub(1).filter(|&i| i < self.accounts.len()) else { return };
        self.accounts[idx].last_msg = now;
        self.accounts[idx].connected = true;
        self.accounts[idx].ever_connected = true;

        match ty {
            "field-init" => {
                let slug = msg.get("slug").map(text).unwrap_or_default();
                let acc = &mut self.accounts[idx];
                acc.in_hunt = true;
                acc.hunt_since = now;
                acc.fainted = false;
                if acc.session.slug != slug {
                    let record = acc.session.record(acc.slot, now);
                    acc.session = Session::new(&slug, now);
                    if let Some(r) = record {
                        self.history.hunts.push_back(r);
                        self.dirty = true;
                    }
                }
            }
            "field-none" | "field-teleport-city" => {
                let acc = &mut self.accounts[idx];
                acc.in_hunt = false;
                acc.fainted = false;
            }
            "field" => {
                self.accounts[idx].fainted = truthy(msg.get("fainted"));
            }
            "field-kill" => self.on_kill(idx, msg, now),
            "catch-result" => self.on_catch_result(idx, msg, now),
            "balls" => self.on_balls(idx, msg),
            "pokes" => {
                let list = msg.get("list").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                self.on_pokes(idx, list, now);
            }
            "pokes-chunk" => {
                if let Some(list) = self.on_chunk(idx, msg) {
                    self.on_pokes(idx, &list, now);
                }
            }
            "poke-delta" => {
                if let Some(pk) = msg.get("poke").filter(|p| p.is_object()) {
                    self.on_poke(idx, pk, now);
                }
            }
            "poke-xp" => {
                let id = poke_id(msg);
                let acc = &mut self.accounts[idx];
                if let Some(mon) = id.and_then(|id| acc.team.get_mut(&id)) {
                    if let Some(l) = msg.get("level").filter(|v| !v.is_null()) {
                        mon.level = num(l);
                    }
                }
            }
            _ => {}
        }
    }

    fn on_kill(&mut self, idx: usize, msg: &Value, now: u64) {
        let a = self.accounts[idx].slot;
        let xp = num(msg.get("xpGained").unwrap_or(&Value::Null)).max(0.0);
        let mut gold = 0.0;
        let mut loot: Vec<(u32, String, f64, f64)> = Vec::new();
        for l in msg.get("loot").and_then(Value::as_array).into_iter().flatten() {
            let id = num(l.get("itemId").unwrap_or(&Value::Null)).max(0.0) as u32;
            let qty = num(l.get("qty").unwrap_or(&Value::Null)).max(0.0);
            if qty <= 0.0 {
                continue;
            }
            let g = qty * self.prices.of(id);
            gold += g;
            loot.push((id, l.get("name").map(text).unwrap_or_default(), qty, g));
        }
        let shiny = truthy(msg.get("shiny"));

        let acc = &mut self.accounts[idx];
        let s = &mut acc.session;
        s.kills += 1;
        s.last_kill = now;
        s.xp += xp;
        s.gold += gold;
        for (id, name, qty, g) in &loot {
            let d = s.drops.entry(*id).or_insert_with(|| (name.clone(), 0.0, 0.0));
            d.1 += qty;
            d.2 += g;
        }
        if shiny {
            s.shiny_seen += 1;
        }

        let b = self.history.bucket(now);
        b.totals.kills += 1;
        b.totals.xp += xp;
        b.totals.gold += gold;
        for (id, name, qty, g) in &loot {
            add_drop(&mut b.drops, a, *id, name, *qty, *g);
        }
        if shiny {
            b.totals.shiny_seen += 1;
            let name = msg.get("speciesName").or_else(|| msg.get("name")).map(text).unwrap_or_default();
            self.history.shinies.push_back(ShinyEntry {
                account: a,
                name,
                species_id: species_of(msg),
                t: now,
                captured: false,
            });
        }
        self.dirty = true;
    }

    fn on_catch_result(&mut self, idx: usize, msg: &Value, now: u64) {
        let ball = num(msg.get("ballId").unwrap_or(&Value::Null)) as i64;
        let acc = &mut self.accounts[idx];
        let (price, infinite) = acc.ball_info.get(&ball).copied().unwrap_or((0.0, false));
        let cost = if infinite { 0.0 } else { price };
        acc.session.balls += 1;
        acc.session.gold -= cost;
        acc.since_catch += 1;
        if let Some(left) = acc.balls_left.as_mut() {
            if left.is_finite() && !infinite {
                *left = (*left - 1.0).max(0.0);
            }
        }
        let success = truthy(msg.get("success"));
        if success {
            acc.session.caps += 1;
            acc.pending_balls = acc.since_catch;
            acc.since_catch = 0;
        }
        let b = self.history.bucket(now);
        b.totals.gold -= cost;
        if success {
            b.totals.caps += 1;
        }
        self.dirty = true;
    }

    fn on_balls(&mut self, idx: usize, msg: &Value) {
        let acc = &mut self.accounts[idx];
        for b in msg.get("catalog").and_then(Value::as_array).into_iter().flatten() {
            let id = num(b.get("id").unwrap_or(&Value::Null)) as i64;
            acc.ball_info.insert(id, (num(b.get("priceGold").unwrap_or(&Value::Null)), truthy(b.get("infinite"))));
        }
        if let Some(counts) = msg.get("counts").and_then(Value::as_object) {
            let mut left = 0.0;
            for (k, v) in counts {
                let q = num(v);
                if q <= 0.0 {
                    continue;
                }
                let infinite = k.parse::<i64>().ok().and_then(|id| acc.ball_info.get(&id)).is_some_and(|i| i.1);
                if infinite {
                    left = f64::INFINITY;
                } else {
                    left += q;
                }
            }
            acc.balls_left = Some(left);
        }
    }

    /// Junta a lista em partes (socket v=2); devolve a lista inteira quando chega a última parte
    fn on_chunk(&mut self, idx: usize, msg: &Value) -> Option<Vec<Value>> {
        let gen = num(msg.get("gen")?) as i64;
        let seq = num(msg.get("seq")?) as usize;
        let total = num(msg.get("total")?) as usize;
        // Sem total a lista sairia vazia e a caixa inteira viraria captura na próxima
        if total == 0 || total > MAX_CHUNKS || seq >= total {
            return None;
        }
        let acc = &mut self.accounts[idx];
        let c = acc.chunks.get_or_insert_with(PokesChunks::default);
        if c.gen != gen || c.total != total {
            *c = PokesChunks { gen, total, parts: BTreeMap::new() };
        }
        c.parts.insert(seq, msg.get("list").and_then(Value::as_array).cloned().unwrap_or_default());
        if c.parts.len() < c.total {
            return None;
        }
        let c = acc.chunks.take()?;
        Some(c.parts.into_values().flatten().collect())
    }

    fn on_pokes(&mut self, idx: usize, list: &[Value], now: u64) {
        // A lista inteira refaz o time (quem saiu do time não vem com team=false)
        self.accounts[idx].team.clear();
        for pk in list {
            self.on_poke(idx, pk, now);
        }
        self.accounts[idx].baselined = true;
    }

    fn on_poke(&mut self, idx: usize, pk: &Value, now: u64) {
        let Some(id) = poke_id(pk) else { return };
        let acc = &mut self.accounts[idx];
        acc.update_team(&id, pk);
        let sid = species_of(pk);
        let is_new = acc.baselined && !acc.known_pokes.contains(&id);
        acc.known_pokes.insert(id);
        if !is_new {
            if sid > 0 {
                acc.species_owned.insert(sid);
            }
            return;
        }
        // Pokémon novo na caixa: captura
        let a = acc.slot;
        let first_of_species = sid > 0 && !acc.species_owned.contains(&sid);
        if sid > 0 {
            acc.species_owned.insert(sid);
        }
        let balls = std::mem::take(&mut acc.pending_balls);
        let sell = num(pk.get("sellValue").unwrap_or(&Value::Null)).max(0.0);
        let shiny = truthy(pk.get("shiny"));
        acc.session.gold += sell;
        if shiny {
            acc.session.shiny_caught += 1;
        }
        let name = pk.get("name").map(text).unwrap_or_else(|| "?".into());

        let b = self.history.bucket(now);
        b.totals.gold += sell;
        if shiny {
            b.totals.shiny_caught += 1;
            // Shiny derrotado há pouco nesta conta: é o mesmo encontro, agora capturado
            let open = self.history.shinies.iter_mut().rev().find(|e| {
                e.account == a && !e.captured && now.saturating_sub(e.t) < SHINY_MATCH_MS && (e.species_id == sid || e.species_id == 0 || sid == 0)
            });
            match open {
                Some(e) => e.captured = true,
                None => {
                    // Captura sem o abate visto (ex.: socket reconectou no meio): conta o encontro também
                    self.history.bucket(now).totals.shiny_seen += 1;
                    self.accounts[idx].session.shiny_seen += 1;
                    self.history.shinies.push_back(ShinyEntry { account: a, name: name.clone(), species_id: sid, t: now, captured: true });
                }
            }
        }
        self.history.catches.push_back(CatchEntry {
            account: a,
            name,
            species_id: sid,
            iv: num(pk.get("ivTotal").unwrap_or(&Value::Null)),
            q: num(pk.get("quality").unwrap_or(&Value::Null)),
            shiny,
            new: first_of_species,
            balls,
            t: now,
        });
        self.history.prune(now);
        self.dirty = true;
    }

    /// Zerar: começa uma sessão nova em todas as contas (o histórico do dia continua)
    pub fn reset_sessions(&mut self, now: u64) {
        for acc in &mut self.accounts {
            if let Some(r) = acc.session.record(acc.slot, now) {
                self.history.hunts.push_back(r);
            }
            let slug = acc.session.slug.clone();
            acc.session = Session::new(&slug, now);
            acc.since_catch = 0;
            acc.pending_balls = 0;
        }
        self.epoch = now;
        self.history.prune(now);
        self.dirty = true;
    }

    pub fn snapshot(&self, now: u64, tz_offset_min: i32) -> Snapshot {
        let today_start = local_day_start(now, tz_offset_min);
        let accounts = self.accounts.iter().map(|acc| account_view(acc, now)).collect();

        let mut drops_today: Vec<DropRow> = Vec::new();
        for (_, b) in self.history.hours.range(today_start..) {
            for d in &b.drops {
                add_drop(&mut drops_today, d.a, d.id, &d.name, d.qty, d.gold);
            }
        }

        let pending = |e: &ShinyEntry| !e.captured && now.saturating_sub(e.t) < SHINY_MATCH_MS;
        let mut shiny_session = [0u64; 3];
        for e in self.history.shinies.iter().filter(|e| e.t >= self.epoch) {
            shiny_session[1] += 1;
            if e.captured {
                shiny_session[0] += 1;
            } else if !pending(e) {
                shiny_session[2] += 1;
            }
        }

        Snapshot {
            now,
            today: self.history.sum_since(today_start),
            week: self.history.sum_since(today_start.saturating_sub(6 * DAY_MS)),
            quarter: self.history.sum_since(today_start.saturating_sub(89 * DAY_MS)),
            accounts,
            catches: self.history.catches.iter().rev().take(300).cloned().collect(),
            shinies: self
                .history
                .shinies
                .iter()
                .rev()
                .take(100)
                .map(|e| ShinyView { entry: e.clone(), pending: pending(e) })
                .collect(),
            shiny_session,
            drops_today,
            hunts_count: self.history.hunts.len(),
        }
    }
}

fn account_view(acc: &Account, now: u64) -> AccountView {
    let s = &acc.session;
    // Fora da hunt a sessão termina no último kill: o tempo parado na cidade não dilui as taxas
    let end = if acc.in_hunt { now } else { s.last_kill.max(s.start) };
    let elapsed_ms = end.saturating_sub(s.start);

    let since_kill = now.saturating_sub(s.last_kill.max(acc.hunt_since));
    let balls_min = match acc.balls_left {
        Some(left) if left.is_finite() && s.balls >= 5 && elapsed_ms >= 5 * 60_000 => {
            let per_min = s.balls as f64 / (elapsed_ms as f64 / 60_000.0);
            Some(left / per_min)
        }
        _ => None,
    };
    let (status, status_min) = if !acc.ever_connected {
        ("waiting", 0.0)
    } else if !acc.connected && now.saturating_sub(acc.closed_at) >= OFFLINE_MS {
        ("offline", 0.0)
    } else if acc.in_hunt && acc.fainted {
        ("fainted", 0.0)
    } else if acc.in_hunt && since_kill >= STALLED_MS {
        ("stalled", since_kill as f64 / 60_000.0)
    } else if let Some(min) = balls_min.filter(|&m| acc.in_hunt && m < BALLS_WARN_MIN) {
        ("balls", min)
    } else if acc.in_hunt {
        ("hunting", 0.0)
    } else {
        ("city", 0.0)
    };

    let leader = acc.leader();
    AccountView {
        slot: acc.slot,
        status,
        status_min,
        pokemon: leader.map(|m| m.name.clone()).unwrap_or_default(),
        level: leader.map(|m| m.level).unwrap_or(0.0),
        in_hunt: acc.in_hunt,
        hunt_slug: if acc.in_hunt { s.slug.clone() } else { String::new() },
        hunt_name: String::new(),
        elapsed_ms,
        gold: s.gold,
        xp: s.xp,
        kills: s.kills,
        caps: s.caps,
        balls: s.balls,
        shiny_seen: s.shiny_seen,
        balls_left: acc.balls_left.filter(|b| b.is_finite()),
    }
}

/// Só o socket do jogo conta (wss://poke.idleworld.online/ws...)
pub fn is_game_socket(url: &str) -> bool {
    let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://"));
    let Some(rest) = rest else { return false };
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    host.eq_ignore_ascii_case("poke.idleworld.online") && path.starts_with("/ws")
}

/// Campo de CSV separado por `;` (Excel em português), com aspas quando precisa
pub fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Número no formato do Excel em português (vírgula decimal)
pub fn csv_num(v: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, v).replace('.', ",")
}

/// Nome legível de um slug de hunt sem catálogo: "rota_7-norte" -> "Rota 7 Norte"
pub fn pretty_slug(slug: &str) -> String {
    slug.split(['_', '-'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Planilha CSV (`;`, com BOM para o Excel abrir em UTF-8): `days` = um dia por linha (90 dias),
/// `hunts` = uma hunt encerrada por linha. Devolve (parte do nome do arquivo, conteúdo)
pub fn export_csv(
    history: &History,
    kind: &str,
    now: u64,
    tz_offset_min: i32,
    account: impl Fn(u8) -> String,
    hunt_name: impl Fn(&str) -> String,
) -> Result<(&'static str, String), String> {
    let mut csv = String::from("\u{feff}");
    match kind {
        "days" => {
            csv.push_str("data;gold;xp;kills;capturas;shiny_encontrados;shiny_capturados\n");
            for (day, t) in history.days(now, tz_offset_min, 90) {
                if t.kills == 0 && t.caps == 0 {
                    continue;
                }
                let date = &local_datetime(day, tz_offset_min)[..10];
                csv.push_str(&format!(
                    "{};{};{};{};{};{};{}\n",
                    date,
                    csv_num(t.gold, 0),
                    csv_num(t.xp, 0),
                    t.kills,
                    t.caps,
                    t.shiny_seen,
                    t.shiny_caught
                ));
            }
            Ok(("dias", csv))
        }
        "hunts" => {
            csv.push_str("conta;hunt;inicio;fim;minutos;kills;xp;gold;capturas;shiny;pokebolas;kills_h;xp_h;gold_h;drops\n");
            for h in history.hunts.iter().rev() {
                let hours = (h.end.saturating_sub(h.start) as f64 / 3_600_000.0).max(1.0 / 60.0);
                let drops =
                    h.drops.iter().map(|d| format!("{} x{}", d.name, csv_num(d.qty, 0))).collect::<Vec<_>>().join(" | ");
                csv.push_str(&format!(
                    "{};{};{};{};{};{};{};{};{};{};{};{};{};{};{}\n",
                    csv_field(&account(h.account)),
                    csv_field(&hunt_name(&h.slug)),
                    local_datetime(h.start, tz_offset_min),
                    local_datetime(h.end, tz_offset_min),
                    csv_num(hours * 60.0, 0),
                    h.kills,
                    csv_num(h.xp, 0),
                    csv_num(h.gold, 0),
                    h.caps,
                    h.shiny_seen,
                    h.balls,
                    csv_num(h.kills as f64 / hours, 0),
                    csv_num(h.xp / hours, 0),
                    csv_num(h.gold / hours, 0),
                    csv_field(&drops)
                ));
            }
            Ok(("hunts", csv))
        }
        other => Err(format!("Exportação desconhecida: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T0: u64 = 1_793_750_400_000; // 2026-11-04 00:00 UTC

    fn state() -> FarmState {
        let mut st = FarmState::new(History::default(), T0);
        st.set_prices(Prices { by_id: HashMap::from([(10, 100.0), (11, 5.0)]) });
        st.socket_created(1, "r1", "wss://poke.idleworld.online/ws?v=2");
        st
    }

    fn msg(st: &mut FarmState, slot: usize, v: Value, t: u64) {
        st.frame(slot, if slot == 1 { "r1" } else { "rx" }, &v.to_string(), t);
    }

    #[test]
    fn socket_do_jogo() {
        assert!(is_game_socket("wss://poke.idleworld.online/ws?v=2"));
        assert!(is_game_socket("wss://POKE.idleworld.online/ws"));
        assert!(!is_game_socket("wss://poke.idleworld.online.evil.com/ws"));
        assert!(!is_game_socket("wss://poke.idleworld.online/outro"));
        assert!(!is_game_socket("https://poke.idleworld.online/ws"));
    }

    #[test]
    fn kill_soma_xp_loot_e_gold() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"field-init","slug":"electabuzz"}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1000,"loot":[{"itemId":10,"name":"Pedra","qty":2},{"itemId":11,"name":"Pena","qty":3}]}), T0 + 6_000);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":500,"loot":[]}), T0 + 12_000);
        let snap = st.snapshot(T0 + 60_000, 0);
        let a = &snap.accounts[0];
        assert_eq!(a.kills, 2);
        assert_eq!(a.xp, 1500.0);
        assert_eq!(a.gold, 215.0);
        assert_eq!(a.status, "hunting");
        assert_eq!(a.hunt_slug, "electabuzz");
        assert_eq!(snap.today.kills, 2);
        assert_eq!(snap.today.gold, 215.0);
        let pedra = snap.drops_today.iter().find(|d| d.id == 10).unwrap();
        assert_eq!((pedra.a, pedra.qty, pedra.gold), (1, 2.0, 200.0));
    }

    #[test]
    fn socket_de_outra_pagina_e_ignorado() {
        let mut st = state();
        st.frame(1, "outro", &json!({"type":"field-kill","xpGained":9}).to_string(), T0);
        assert_eq!(st.snapshot(T0, 0).accounts[0].kills, 0);
        // Conta sem socket conhecido adota o primeiro que mandar mensagem do jogo
        st.frame(2, "r2", &json!({"type":"field-kill","xpGained":9}).to_string(), T0);
        assert_eq!(st.snapshot(T0, 0).accounts[1].kills, 1);
    }

    #[test]
    fn captura_so_depois_da_primeira_lista() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":1,"name":"Pidgey","speciesId":16}}), T0);
        assert!(st.history.catches.is_empty(), "antes da lista da caixa não dá para saber o que é novo");
        msg(&mut st, 1, json!({"type":"pokes","list":[{"id":1,"name":"Pidgey","speciesId":16},{"id":2,"name":"Abra","speciesId":63,"team":true,"leader":true,"level":40,"slot":0}]}), T0);
        assert!(st.history.catches.is_empty());
        msg(&mut st, 1, json!({"type":"balls","catalog":[{"id":1,"priceGold":10},{"id":9,"priceGold":0,"infinite":true}],"counts":{"1":100}}), T0);
        msg(&mut st, 1, json!({"type":"catch-result","ballId":1,"success":false}), T0 + 1_000);
        msg(&mut st, 1, json!({"type":"catch-result","ballId":1,"success":true}), T0 + 2_000);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":3,"name":"Pinsir","speciesId":127,"ivTotal":108,"quality":"1.76","sellValue":500}}), T0 + 2_100);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":1,"name":"Pidgey","speciesId":16,"level":5}}), T0 + 3_000);

        let c = &st.history.catches;
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].name.as_str(), c[0].iv, c[0].q, c[0].new, c[0].balls), ("Pinsir", 108.0, 1.76, true, 2));
        let snap = st.snapshot(T0 + 10_000, 0);
        let a = &snap.accounts[0];
        assert_eq!((a.caps, a.balls), (1, 2));
        assert_eq!(a.gold, 500.0 - 20.0, "venda da captura menos as 2 pokébolas");
        assert_eq!(a.balls_left, Some(98.0));
        assert_eq!((a.pokemon.as_str(), a.level), ("Abra", 40.0));
        assert_eq!(snap.today.caps, 1);
    }

    #[test]
    fn segunda_do_especie_nao_e_nova() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"pokes","list":[{"id":1,"name":"Pidgey","speciesId":16}]}), T0);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":2,"name":"Pidgey","speciesId":16}}), T0);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":3,"name":"Rattata","speciesId":19}}), T0);
        let c: Vec<bool> = st.history.catches.iter().map(|c| c.new).collect();
        assert_eq!(c, vec![false, true]);
    }

    #[test]
    fn lista_em_partes() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"pokes-chunk","gen":7,"seq":1,"total":2,"list":[{"id":2,"speciesId":63}]}), T0);
        assert!(!st.accounts[0].baselined);
        msg(&mut st, 1, json!({"type":"pokes-chunk","gen":7,"seq":0,"total":2,"list":[{"id":1,"speciesId":16}]}), T0);
        assert!(st.accounts[0].baselined);
        assert_eq!(st.accounts[0].known_pokes.len(), 2);
        // Total 0 nunca fecha a lista (a caixa inteira viraria captura)
        let mut st = state();
        msg(&mut st, 1, json!({"type":"pokes-chunk","gen":1,"seq":0,"total":0,"list":[]}), T0);
        assert!(!st.accounts[0].baselined);
    }

    #[test]
    fn shiny_derrotado_e_capturado_e_um_encontro() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"pokes","list":[]}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":10,"loot":[],"shiny":true,"speciesId":19,"speciesName":"Rattata"}), T0);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":70,"name":"Rattata","speciesId":19,"shiny":true}}), T0 + 4_000);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":10,"loot":[],"shiny":true,"speciesId":4,"speciesName":"Charmander"}), T0 + 5_000);
        let snap = st.snapshot(T0 + 120_000, 0);
        assert_eq!(snap.today.shiny_seen, 2);
        assert_eq!(snap.today.shiny_caught, 1);
        assert_eq!(snap.shiny_session, [1, 2, 1]);
        assert_eq!(snap.shinies.len(), 2);
        assert!(snap.shinies[1].entry.captured && !snap.shinies[0].entry.captured);
        // Ainda dentro do minuto: não conta como perdido
        assert_eq!(st.snapshot(T0 + 30_000, 0).shiny_session, [1, 2, 0]);
    }

    #[test]
    fn troca_de_hunt_grava_a_anterior() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"field-init","slug":"a"}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":10,"loot":[{"itemId":10,"name":"Pedra","qty":1}]}), T0 + 60_000);
        // Reconectar na mesma hunt não fecha a sessão
        msg(&mut st, 1, json!({"type":"field-init","slug":"a"}), T0 + 70_000);
        assert!(st.history.hunts.is_empty());
        msg(&mut st, 1, json!({"type":"field-init","slug":"b"}), T0 + 3_600_000);
        let h = &st.history.hunts[0];
        assert_eq!((h.slug.as_str(), h.kills, h.start, h.end), ("a", 1, T0, T0 + 60_000));
        assert_eq!(h.drops, vec![HuntDrop { name: "Pedra".into(), qty: 1.0, gold: 100.0 }]);
        // Hunt sem kill não vira registro
        msg(&mut st, 1, json!({"type":"field-init","slug":"c"}), T0 + 3_700_000);
        assert_eq!(st.history.hunts.len(), 1);
    }

    #[test]
    fn status_da_conta() {
        let mut st = state();
        assert_eq!(st.snapshot(T0, 0).accounts[1].status, "waiting");
        msg(&mut st, 1, json!({"type":"field-init","slug":"a"}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1}), T0);
        assert_eq!(st.snapshot(T0 + 11 * 60_000, 0).accounts[0].status, "stalled");
        msg(&mut st, 1, json!({"type":"field","fainted":true}), T0 + 60_000);
        assert_eq!(st.snapshot(T0 + 61_000, 0).accounts[0].status, "fainted");
        msg(&mut st, 1, json!({"type":"field-teleport-city"}), T0 + 62_000);
        assert_eq!(st.snapshot(T0 + 63_000, 0).accounts[0].status, "city");
        st.socket_closed(1, "r1", T0 + 64_000);
        assert_eq!(st.snapshot(T0 + 70_000, 0).accounts[0].status, "city", "queda curta: o jogo reconecta");
        assert_eq!(st.snapshot(T0 + 90_000, 0).accounts[0].status, "offline");
    }

    #[test]
    fn aviso_de_pokebolas() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"balls","catalog":[{"id":1,"priceGold":0}],"counts":{"1":30}}), T0);
        msg(&mut st, 1, json!({"type":"field-init","slug":"a"}), T0);
        // 10 bolas em 10 min = 1 por minuto; sobram 20 -> ~20 min
        for i in 0..10 {
            msg(&mut st, 1, json!({"type":"field-kill","xpGained":1}), T0 + i * 60_000);
            msg(&mut st, 1, json!({"type":"catch-result","ballId":1,"success":false}), T0 + i * 60_000);
        }
        let a = &st.snapshot(T0 + 10 * 60_000, 0).accounts[0];
        assert_eq!(a.status, "balls");
        assert!((a.status_min - 20.0).abs() < 0.01, "{}", a.status_min);
    }

    #[test]
    fn zerar_comeca_sessao_nova_e_mantem_o_dia() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"field-init","slug":"a"}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":10}), T0 + 1_000);
        st.reset_sessions(T0 + 2_000);
        let snap = st.snapshot(T0 + 3_000, 0);
        assert_eq!(snap.accounts[0].kills, 0);
        assert_eq!(snap.accounts[0].hunt_slug, "a");
        assert_eq!(snap.today.kills, 1);
        assert_eq!(snap.hunts_count, 1);
    }

    #[test]
    fn dia_local_e_periodos() {
        // 2026-11-04 02:00 UTC = 2026-11-03 23:00 em Brasília (offset 180)
        let t = T0 + 2 * HOUR_MS;
        assert_eq!(local_day_start(t, 180), T0 - DAY_MS + 3 * HOUR_MS);
        assert_eq!(local_day_start(t, 0), T0);
        assert_eq!(local_datetime(t, 180), "2026-11-03 23:00");
        assert_eq!(local_datetime(T0 + 9 * HOUR_MS + 57 * 60_000, 0), "2026-11-04 09:57");

        let mut st = state();
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1}), T0 - 3 * DAY_MS);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1}), T0 - 30 * DAY_MS);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1}), T0 + HOUR_MS);
        let snap = st.snapshot(T0 + 2 * HOUR_MS, 0);
        assert_eq!((snap.today.kills, snap.week.kills, snap.quarter.kills), (1, 2, 3));
        let days = st.history.days(T0 + 2 * HOUR_MS, 0, 90);
        assert_eq!(days[0], (T0, Totals { xp: 1.0, kills: 1, ..Default::default() }));
        assert_eq!(days[3].1.kills, 1);
    }

    #[test]
    fn poda_do_historico() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1,"loot":[{"itemId":10,"name":"Pedra","qty":1}]}), T0 - 100 * DAY_MS);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":1,"loot":[{"itemId":10,"name":"Pedra","qty":1}]}), T0 - 10 * DAY_MS);
        st.history.prune(T0);
        assert_eq!(st.history.hours.len(), 1, "mais de 92 dias sai");
        assert!(st.history.hours.values().all(|b| b.drops.is_empty()), "drops por item só nos últimos 8 dias");
    }

    #[test]
    fn historico_sobrevive_ao_disco() {
        let mut st = state();
        msg(&mut st, 1, json!({"type":"pokes","list":[]}), T0);
        msg(&mut st, 1, json!({"type":"field-kill","xpGained":5,"loot":[{"itemId":10,"name":"Pedra","qty":1}]}), T0);
        msg(&mut st, 1, json!({"type":"poke-delta","poke":{"id":9,"name":"Abra","speciesId":63,"quality":"1,25"}}), T0);
        let json = serde_json::to_string(&st.history).unwrap();
        let back: History = serde_json::from_str(&json).unwrap();
        assert_eq!(back.hours.len(), 1);
        assert_eq!(back.catches[0].q, 1.25);
        let st2 = FarmState::new(back, T0);
        assert_eq!(st2.snapshot(T0 + 1, 0).today.xp, 5.0);
    }

    #[test]
    fn csv() {
        assert_eq!(csv_field("a;b"), "\"a;b\"");
        assert_eq!(csv_field("Pedra"), "Pedra");
        assert_eq!(csv_num(1234.567, 1), "1234,6");
    }

    #[test]
    fn pretty_slug_capitalizes_words() {
        assert_eq!(pretty_slug("rota_7-norte"), "Rota 7 Norte");
        assert_eq!(pretty_slug(""), "");
    }

    #[test]
    fn export_csv_days_and_hunts() {
        let now = 1_700_000_000_000;
        let mut st = FarmState::new(History::default(), now);
        st.handle(1, &json!({"type": "field-init", "slug": "rota_1"}), now);
        st.handle(1, &json!({"type": "field-kill", "xpGained": 50, "loot": []}), now + 1000);
        st.reset_sessions(now + 2000);

        let (label, csv) = export_csv(&st.history, "days", now + 3000, 180, |a| format!("A{}", a), pretty_slug).unwrap();
        assert_eq!(label, "dias");
        assert!(csv.starts_with('\u{feff}'));
        assert_eq!(csv.lines().count(), 2, "{}", csv);
        assert!(csv.lines().nth(1).unwrap().ends_with(";0;50;1;0;0;0"), "{}", csv);

        let (label, csv) = export_csv(&st.history, "hunts", now + 3000, 180, |a| format!("A;{}", a), pretty_slug).unwrap();
        assert_eq!(label, "hunts");
        assert!(csv.lines().nth(1).unwrap().starts_with("\"A;1\";Rota 1;"), "{}", csv);

        assert!(export_csv(&st.history, "x", now, 0, |_| String::new(), pretty_slug).is_err());
    }
}
