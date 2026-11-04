//! Modelo de rendimento em hunt (XP/h e gold/h) usado pela Tierlist e pelo "Onde caçar".
//! Portado do PokeGrid original (MIT), sem a calibração por medição: ela depende de ler o estado do jogo,
//! o que este app não faz. Ficam os padrões medidos pelo autor (2,1 s por golpe + 3,1 s por kill).
use super::catalog::{Catalog, Hunt, Species};
use super::iv_math::{self, IvResult};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const TYPES: [&str; 18] = [
    "NORMAL", "FIRE", "WATER", "ELECTRIC", "GRASS", "ICE", "FIGHTING", "POISON", "GROUND", "FLYING", "PSYCHIC",
    "BUG", "ROCK", "GHOST", "DRAGON", "DARK", "STEEL", "FAIRY",
];

/// Tabela de efetividade do jogo: (atacante, defensor) -> multiplicador; o que não está aqui vale 1
const CHART: &[(&str, &[(&str, f64)])] = &[
    ("NORMAL", &[("ROCK", 0.5), ("GHOST", 0.0), ("STEEL", 0.5)]),
    ("FIRE", &[("FIRE", 0.5), ("WATER", 0.5), ("GRASS", 2.0), ("ICE", 2.0), ("BUG", 2.0), ("ROCK", 0.5), ("DRAGON", 0.5), ("STEEL", 2.0)]),
    ("WATER", &[("FIRE", 2.0), ("WATER", 0.5), ("GRASS", 0.5), ("GROUND", 2.0), ("ROCK", 2.0), ("DRAGON", 0.5)]),
    ("ELECTRIC", &[("WATER", 2.0), ("ELECTRIC", 0.5), ("GRASS", 0.5), ("GROUND", 0.0), ("FLYING", 2.0), ("DRAGON", 0.5)]),
    ("GRASS", &[("FIRE", 0.5), ("WATER", 2.0), ("GRASS", 0.5), ("POISON", 0.5), ("GROUND", 2.0), ("FLYING", 0.5), ("BUG", 0.5), ("ROCK", 2.0), ("DRAGON", 0.5), ("STEEL", 0.5)]),
    ("ICE", &[("FIRE", 0.5), ("WATER", 0.5), ("GRASS", 2.0), ("ICE", 0.5), ("GROUND", 2.0), ("FLYING", 2.0), ("DRAGON", 2.0), ("STEEL", 0.5)]),
    ("FIGHTING", &[("NORMAL", 2.0), ("ICE", 2.0), ("POISON", 0.5), ("FLYING", 0.5), ("PSYCHIC", 0.5), ("BUG", 0.5), ("ROCK", 2.0), ("GHOST", 0.0), ("DARK", 2.0), ("STEEL", 2.0), ("FAIRY", 0.5)]),
    ("POISON", &[("GRASS", 2.0), ("POISON", 0.5), ("GROUND", 0.5), ("ROCK", 0.5), ("GHOST", 0.5), ("STEEL", 0.0), ("FAIRY", 2.0)]),
    ("GROUND", &[("FIRE", 2.0), ("ELECTRIC", 2.0), ("GRASS", 0.5), ("POISON", 2.0), ("FLYING", 0.0), ("BUG", 0.5), ("ROCK", 2.0), ("STEEL", 2.0)]),
    ("FLYING", &[("ELECTRIC", 0.5), ("GRASS", 2.0), ("FIGHTING", 2.0), ("BUG", 2.0), ("ROCK", 0.5), ("STEEL", 0.5)]),
    ("PSYCHIC", &[("FIGHTING", 2.0), ("POISON", 2.0), ("PSYCHIC", 0.5), ("DARK", 0.0), ("STEEL", 0.5)]),
    ("BUG", &[("FIRE", 0.5), ("GRASS", 2.0), ("FIGHTING", 0.5), ("POISON", 0.5), ("FLYING", 0.5), ("PSYCHIC", 2.0), ("GHOST", 0.5), ("DARK", 2.0), ("STEEL", 0.5), ("FAIRY", 0.5)]),
    ("ROCK", &[("FIRE", 2.0), ("ICE", 2.0), ("FIGHTING", 0.5), ("GROUND", 0.5), ("FLYING", 2.0), ("BUG", 2.0), ("STEEL", 0.5)]),
    ("GHOST", &[("NORMAL", 0.0), ("PSYCHIC", 2.0), ("GHOST", 2.0), ("DARK", 0.5)]),
    ("DRAGON", &[("DRAGON", 2.0), ("STEEL", 0.5), ("FAIRY", 0.0)]),
    ("DARK", &[("FIGHTING", 0.5), ("PSYCHIC", 2.0), ("GHOST", 2.0), ("DARK", 0.5), ("FAIRY", 0.5)]),
    ("STEEL", &[("FIRE", 0.5), ("WATER", 0.5), ("ELECTRIC", 0.5), ("ICE", 2.0), ("ROCK", 2.0), ("STEEL", 0.5), ("FAIRY", 2.0)]),
    ("FAIRY", &[("FIRE", 0.5), ("FIGHTING", 2.0), ("POISON", 0.5), ("DRAGON", 2.0), ("DARK", 2.0), ("STEEL", 0.5)]),
];

/// Disco elemental de cada tipo: (nome, físico?). Poder 300, a cada 10 s, só com 2+ selvagens perto
const TM_TYPES: &[(&str, &str, bool)] = &[
    ("NORMAL", "Omega Clap", true), ("GHOST", "Untold Nightmare", false), ("GRASS", "Petal Bloom", false),
    ("POISON", "Toxic Deluge", true), ("FIRE", "Ignition Point", false), ("FLYING", "Massive Hurricane", true),
    ("WATER", "Cyclonic Tide", false), ("ELECTRIC", "Static Overload", false), ("PSYCHIC", "Mental Singularity", false),
    ("GROUND", "Crossing Fissure", true), ("FIGHTING", "Meteor Fists", true), ("ROCK", "Crumbling Rain", true),
    ("ICE", "Ice Time", false), ("BUG", "Hive Crush", true), ("DARK", "Endless Hollow", true),
    ("DRAGON", "Draconic Soul", false), ("FAIRY", "Lovely Expulsion", false), ("STEEL", "Steel Flare", true),
];

/// Abaixo disso (kills esperados por golpe) a hunt é inviável para o atacante
const RITMO_MIN: f64 = 0.15;
/// Constante de escala do dano (não é uma calibração com dado real; ver PokeGrid original)
const K_DANO: f64 = 0.1;
/// Espalhamento log-normal do dano real em volta do modelado
const SIG_DANO: f64 = 0.5;
const TEMPO_GOLPE_S: f64 = 2.1;
const TEMPO_KILL_S: f64 = 3.1;
/// Sem medição: 2 alvos por salva do disco e 360 salvas/h (recarga cheia)
const TM_ALVOS: f64 = 2.0;
const TM_SALVAS_H: f64 = 360.0;
const AOE_ALVOS: f64 = 2.0;

/// Multiplicador de um tipo de golpe contra os tipos do defensor (tabela crua do jogo)
pub fn type_effectiveness(attack: &str, defender: &[String]) -> f64 {
    let row = CHART.iter().find(|(t, _)| *t == attack).map(|(_, r)| *r).unwrap_or(&[]);
    defender
        .iter()
        .map(|d| row.iter().find(|(t, _)| t == d).map_or(1.0, |(_, m)| *m))
        .product()
}

/// Na hunt a vantagem elemental é amplificada: x2 vira x2,5, x4 vira x5,5; resistências dividem por 1,5
pub fn amplify(e: f64) -> f64 {
    if e > 1.0 {
        1.0 + (e - 1.0) * 1.5
    } else if e < 1.0 {
        e / 1.5
    } else {
        e
    }
}

fn erf(z: f64) -> f64 {
    let s = if z < 0.0 { -1.0 } else { 1.0 };
    let z = z.abs();
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let y = 1.0
        - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592)
            * t
            * (-z * z).exp();
    s * y
}

struct Tabs {
    ritmo: Vec<f64>,
    golpes: Vec<f64>,
}

/// Kills esperados por golpe e golpes esperados por kill, com o dano real = modelado x fator log-normal
fn tabs() -> &'static Tabs {
    static TABS: OnceLock<Tabs> = OnceLock::new();
    TABS.get_or_init(|| {
        let f = |v: f64| 0.5 * (1.0 - erf(v.ln() / SIG_DANO / std::f64::consts::SQRT_2));
        let mut ritmo = Vec::with_capacity(185);
        let mut golpes = Vec::with_capacity(185);
        for i in 0..=184 {
            let mg = (-4.6 + i as f64 * 0.05).exp();
            let (mut r, mut e, mut prev) = (0.0, 0.0, 0.0);
            for k in 1..=1000 {
                let cur = f(1.0 / (k as f64 * mg));
                r += (cur - prev) / k as f64;
                e += (cur - prev) * k as f64;
                prev = cur;
            }
            ritmo.push(r + (1.0 - prev) / 1000.0);
            golpes.push(e + (1.0 - prev) * 1000.0);
        }
        Tabs { ritmo, golpes }
    })
}

fn interp(tab: &[f64], mg: f64, below: impl Fn(f64) -> f64) -> f64 {
    let p = (mg.ln() + 4.6) / 0.05;
    if p <= 0.0 {
        return below(tab[0]);
    }
    if p >= 184.0 {
        return tab[184];
    }
    let i = p.floor() as usize;
    let f = p - i as f64;
    tab[i] * (1.0 - f) + tab[i + 1] * f
}

/// Kills esperados por golpe (0..1) para uma folga `mg` (dano / vida do selvagem)
pub fn ritmo(mg: f64) -> f64 {
    if !(mg > 0.0) {
        return 0.0;
    }
    interp(&tabs().ritmo, mg, |t0| t0 * mg / (-4.6f64).exp())
}

/// Golpes esperados por kill (>= 1)
pub fn golpes(mg: f64) -> f64 {
    if !(mg > 0.0) {
        return 1000.0;
    }
    interp(&tabs().golpes, mg, |t0| t0 * (-4.6f64).exp() / mg)
}

fn tempo_kill(golpes: f64) -> f64 {
    TEMPO_GOLPE_S * golpes.max(1.0) + TEMPO_KILL_S
}

/// Discos elementais que a espécie pode aprender (estágio final, do próprio tipo)
fn tm_discs(sp: &Species) -> Vec<&'static str> {
    if !sp.is_final {
        return Vec::new();
    }
    sp.types
        .iter()
        .filter_map(|t| TM_TYPES.iter().find(|(tt, _, _)| tt == t).map(|(tt, _, _)| *tt))
        .collect()
}

#[derive(Debug, Clone)]
pub enum Tms {
    None,
    /// Tierlist "com TM": o melhor disco que a espécie aprende + AoE se for estágio final
    All,
    List(Vec<String>),
}

pub struct Attacker<'a> {
    pub species: &'a Species,
    pub level: f64,
    pub quality: f64,
    pub iv: IvResult,
    pub tms: Tms,
}

impl<'a> Attacker<'a> {
    /// Atacante "padrão" (qualidade 1, IV mediano), como a tierlist avalia
    pub fn standard(species: &'a Species, level: f64, tms: Tms) -> Self {
        Attacker { species, level, quality: 1.0, iv: iv_math::scenario_from_total(Some(iv_math::IV_TOTAL_MEDIO)), tms }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sug {
    pub ritmo: f64,
    pub golpes: f64,
    pub kills_h: f64,
    pub xp_h: f64,
    pub gold_h: f64,
    #[serde(rename = "move")]
    pub move_name: String,
    pub eff: f64,
    pub physical: bool,
    /// Folga: dano do golpe sobre a vida do selvagem (1 = mata de 1 no limite)
    pub folga: f64,
    pub tm: String,
}

/// Atributos do selvagem de cada hunt (qualidade 1, IV mediano, no nível da hunt)
pub struct Defenders<'c> {
    cat: &'c Catalog,
    stats: HashMap<usize, [u32; 6]>,
}

impl<'c> Defenders<'c> {
    pub fn new(cat: &'c Catalog) -> Self {
        let iv = iv_math::scenario_from_total(Some(iv_math::IV_TOTAL_MEDIO));
        let stats = cat
            .hunts
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                let sp = cat.species_by_name(&h.species)?;
                let lv = hunt_level(h, Some(sp));
                iv_math::project(&sp.bases, &iv, 1.0, lv).map(|p| (i, p.stats))
            })
            .collect();
        Defenders { cat, stats }
    }
}

fn hunt_level(h: &Hunt, sp: Option<&Species>) -> f64 {
    if h.level > 0.0 { h.level } else { sp.map_or(1.0, |s| s.hunt_level) }.max(1.0)
}

/// Rendimento de um atacante numa hunt: melhor golpe aprendido, disco elemental e AoE como o jogo faz
pub fn sug_calc(a: &Attacker, hunt_idx: usize, defs: &Defenders) -> Option<Sug> {
    let hunt = defs.cat.hunts.get(hunt_idx)?;
    let ds = defs.stats.get(&hunt_idx)?;
    let a_stats = iv_math::project(&a.species.bases, &a.iv, a.quality, a.level)?.stats;
    let hp_def = ds[0].max(1) as f64;
    let (atk, spa) = (a_stats[1] as f64, a_stats[3] as f64);
    let (def, spd) = (ds[2].max(1) as f64, ds[4].max(1) as f64);

    let mut best: Option<(f64, &str, f64, bool)> = None;
    for m in &a.species.moves {
        if m.learn_level > a.level || !m.tm.is_empty() || !(m.power > 0.0) || !(m.cooldown_ms > 0.0) {
            continue;
        }
        let eff = amplify(type_effectiveness(&m.ty, &hunt.types));
        let stab = if a.species.types.contains(&m.ty) { 1.5 } else { 1.0 };
        let (ofn, dfn) = if m.physical { (atk, def) } else { (spa, spd) };
        let dano = K_DANO * m.power * eff * stab * (ofn / dfn);
        if !(dano > 0.0) {
            continue;
        }
        let rt = dano / hp_def;
        if best.map_or(true, |(b, _, be, _)| rt > b || (rt == b && eff > be)) {
            best = Some((rt, &m.name, eff, m.physical));
        }
    }
    let (rt, move_name, eff, physical) = best?;
    let golpes = golpes(rt);

    let discs = tm_discs(a.species);
    let owned: Vec<String> = match &a.tms {
        Tms::None => Vec::new(),
        Tms::All => discs.iter().map(|d| d.to_string()).chain(a.species.is_final.then(|| "AOE".into())).collect(),
        Tms::List(l) => l.iter().map(|t| t.to_uppercase()).collect(),
    };
    let mut tm_best: Option<(f64, &str)> = None;
    for d in &discs {
        if !owned.iter().any(|o| o == d) {
            continue;
        }
        let physical_tm = TM_TYPES.iter().find(|(t, _, _)| t == d).map_or(false, |x| x.2);
        let eff_t = amplify(type_effectiveness(d, &hunt.types));
        let (ofn, dfn) = if physical_tm { (atk, def) } else { (spa, spd) };
        let mg = K_DANO * 300.0 * eff_t * 1.5 * (ofn / dfn) / hp_def;
        if mg > 0.0 && tm_best.map_or(true, |(b, _)| mg > b) {
            tm_best = Some((mg, d));
        }
    }
    let aoe = a.species.is_final && owned.iter().any(|o| o == "AOE");
    let kh_tm = tm_best.map_or(0.0, |(mg, _)| TM_SALVAS_H * TM_ALVOS * ritmo(mg));
    let alvos = if aoe { AOE_ALVOS } else { 1.0 };
    let kills_h = alvos * 3600.0 / tempo_kill(golpes) + kh_tm;
    let tm = match (tm_best, aoe) {
        (Some((_, d)), true) => format!("{}+AOE", d),
        (Some((_, d)), false) => d.to_string(),
        (None, true) => "AOE".into(),
        (None, false) => String::new(),
    };
    Some(Sug {
        ritmo: ritmo(rt),
        golpes,
        kills_h,
        xp_h: kills_h * hunt.xp,
        gold_h: kills_h * hunt.gold_per_kill,
        move_name: move_name.to_string(),
        eff,
        physical,
        folga: rt,
        tm,
    })
}

fn score(s: &Sug, gold: bool) -> f64 {
    if gold { s.gold_h } else { s.xp_h }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuntRef {
    /// Id do selvagem (sprite)
    pub id: u32,
    pub name: String,
    pub level: f64,
    pub area: String,
    pub types: Vec<String>,
}

impl From<&Hunt> for HuntRef {
    fn from(h: &Hunt) -> Self {
        HuntRef { id: h.species_id, name: h.name.clone(), level: h.level, area: h.area.clone(), types: h.types.clone() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRow {
    pub species: String,
    /// Espécie-base quando a linha é uma forma com prefixo (Mad Golem -> Golem)
    pub base: String,
    pub id: u32,
    pub types: Vec<String>,
    /// Rendimento na melhor hunt
    pub score: f64,
    /// Rendimento somado em todas as hunts (nota da aba Geral)
    pub total: f64,
    pub total_tm: String,
    pub sug: Sug,
    pub hunt: HuntRef,
    /// Melhor hunt de Outland quando a melhor geral é de Orre
    pub outland: Option<(HuntRef, f64)>,
}

/// Ranking de todas as espécies: cada uma avaliada no nível de cada hunt alcançável, com a melhor hunt
pub fn tier_list(cat: &Catalog, level: f64, with_tm: bool, gold: bool) -> Vec<TierRow> {
    let defs = Defenders::new(cat);
    let mut rows: Vec<TierRow> = Vec::new();
    for sp in &cat.species {
        if level > 0.0 && sp.min_level > level {
            continue;
        }
        let discs = if with_tm { tm_discs(sp) } else { Vec::new() };
        let mut sums = vec![0.0; discs.len()];
        let mut soma = 0.0;
        let mut best: Option<(f64, Sug, usize)> = None;
        let mut best_out: Option<(f64, usize)> = None;
        for (hi, h) in cat.hunts.iter().enumerate() {
            if level > 0.0 && h.level > level {
                continue;
            }
            let lv = hunt_level(h, cat.species_by_name(&h.species));
            let tms = if with_tm { Tms::All } else { Tms::None };
            let Some(sg) = sug_calc(&Attacker::standard(sp, lv, tms), hi, &defs) else { continue };
            if sg.ritmo < RITMO_MIN {
                continue;
            }
            let sc = score(&sg, gold);
            if discs.is_empty() {
                soma += sc;
            } else {
                // O jogo deixa 1 disco elemental por pokémon (o AoE convive): soma por disco, fica a maior
                for (k, d) in discs.iter().enumerate() {
                    let tms = Tms::List(vec![d.to_string(), "AOE".into()]);
                    if let Some(s1) = sug_calc(&Attacker::standard(sp, lv, tms), hi, &defs) {
                        sums[k] += score(&s1, gold);
                    }
                }
            }
            if h.area == "outland" && best_out.map_or(true, |(b, _)| sc > b) {
                best_out = Some((sc, hi));
            }
            if best.as_ref().map_or(true, |(b, bs, _)| sc > *b || (sc == *b && sg.folga > bs.folga)) {
                best = Some((sc, sg, hi));
            }
        }
        let Some((sc, sg, hi)) = best.filter(|(sc, _, _)| *sc > 0.0) else { continue };
        let (total, total_tm) = if discs.is_empty() {
            (soma, sg.tm.clone())
        } else {
            let (k, v) = sums.iter().enumerate().fold((0, f64::MIN), |acc, (k, &v)| if v > acc.1 { (k, v) } else { acc });
            (v, format!("{}{}", discs[k], if sg.tm.contains("AOE") { "+AOE" } else { "" }))
        };
        let hunt = &cat.hunts[hi];
        rows.push(TierRow {
            species: sp.name.clone(),
            base: sp.name.clone(),
            id: sp.id,
            types: sp.types.clone(),
            score: sc,
            total,
            total_tm,
            outland: best_out
                .filter(|_| hunt.area == "orre")
                .map(|(s, i)| (HuntRef::from(&cat.hunts[i]), s)),
            sug: sg,
            hunt: hunt.into(),
        });
    }

    // Formas com prefixo (Mad Golem, Furious Scyther...) contam 1x, como a espécie base, quando o tipo é o mesmo
    let canon = |name: &str| -> String {
        let w: Vec<&str> = name.split(' ').collect();
        (1..w.len())
            .map(|k| w[k..].join(" "))
            .find(|rest| cat.species_by_name(rest).is_some())
            .unwrap_or_else(|| name.to_string())
    };
    let mut by_base: HashMap<String, TierRow> = HashMap::new();
    for mut r in rows {
        let b0 = canon(&r.species);
        let same_type = cat.species_by_name(&b0).is_some_and(|b| b.types == r.types);
        let base = if same_type { b0 } else { r.species.clone() };
        let key = base.to_lowercase();
        if by_base.get(&key).map_or(true, |prev| r.score > prev.score) {
            if let Some(b) = cat.species_by_name(&base) {
                r.base = b.name.clone();
                r.id = b.id;
            }
            by_base.insert(key, r);
        }
    }
    let mut out: Vec<TierRow> = by_base.into_values().collect();
    out.sort_by(|p, q| q.score.total_cmp(&p.score).then(q.sug.eff.total_cmp(&p.sug.eff)));
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HuntRow {
    pub hunt: HuntRef,
    pub species: String,
    pub id: u32,
    pub score: f64,
    pub sug: Sug,
}

/// Hunts com selvagem do elemento (1º ou 2º tipo) que o nível alcança, cada uma com o melhor atacante nesse nível
pub fn hunts_by_type(cat: &Catalog, element: &str, level: f64, with_tm: bool, gold: bool) -> Vec<HuntRow> {
    let defs = Defenders::new(cat);
    let lv = level.max(1.0);
    let candidates: Vec<&Species> = cat.species.iter().filter(|s| !(s.min_level > level)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for (hi, h) in cat.hunts.iter().enumerate() {
        if !h.types.iter().any(|t| t == element) || h.level > level || !seen.insert(format!("{}@{}", h.name, h.level)) {
            continue;
        }
        let mut best: Option<(f64, Sug, &Species)> = None;
        for sp in &candidates {
            let tms = if with_tm { Tms::All } else { Tms::None };
            let Some(sg) = sug_calc(&Attacker::standard(sp, lv, tms), hi, &defs) else { continue };
            if sg.ritmo < RITMO_MIN {
                continue;
            }
            let sc = score(&sg, gold);
            if best.as_ref().map_or(true, |(b, bs, _)| sc > *b || (sc == *b && sg.folga > bs.folga)) {
                best = Some((sc, sg, sp));
            }
        }
        if let Some((sc, sg, sp)) = best.filter(|(sc, _, _)| *sc > 0.0) {
            rows.push(HuntRow { hunt: h.into(), species: sp.name.clone(), id: sp.id, score: sc, sug: sg });
        }
    }
    rows.sort_by(|p, q| q.score.total_cmp(&p.score));
    rows
}

/// Um pokémon (com nível, qualidade e IV reais) contra todas as hunts até `max_hunt_level` (0 = todas)
pub fn hunts_for_pokemon(cat: &Catalog, a: &Attacker, max_hunt_level: f64, gold: bool) -> Vec<HuntRow> {
    let defs = Defenders::new(cat);
    let mut rows: Vec<HuntRow> = cat
        .hunts
        .iter()
        .enumerate()
        .filter(|(_, h)| !(max_hunt_level > 0.0 && h.level > max_hunt_level))
        .filter_map(|(hi, h)| {
            let sg = sug_calc(a, hi, &defs).filter(|s| s.ritmo >= RITMO_MIN)?;
            let sc = score(&sg, gold);
            (sc > 0.0).then(|| HuntRow { hunt: h.into(), species: a.species.name.clone(), id: a.species.id, score: sc, sug: sg })
        })
        .collect();
    rows.sort_by(|p, q| q.score.total_cmp(&p.score).then(q.sug.folga.total_cmp(&p.sug.folga)));
    rows
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMatchups {
    /// Tipos de golpe que batem no pokémon com o multiplicador da hunt (só os != 1)
    pub defense: Vec<(String, f64)>,
    /// Para cada tipo do pokémon (STAB): contra quem ele bate forte, fraco ou não afeta
    pub offense: Vec<(String, Vec<(String, f64)>)>,
}

pub fn matchups(types: &[String]) -> TypeMatchups {
    let mut defense: Vec<(String, f64)> = TYPES
        .iter()
        .map(|t| (t.to_string(), amplify(type_effectiveness(t, types))))
        .filter(|(_, m)| (*m - 1.0).abs() > 1e-9)
        .collect();
    defense.sort_by(|a, b| b.1.total_cmp(&a.1));
    let offense = types
        .iter()
        .map(|t| {
            let mut v: Vec<(String, f64)> = TYPES
                .iter()
                .map(|d| (d.to_string(), amplify(type_effectiveness(t, &[d.to_string()]))))
                .filter(|(_, m)| (*m - 1.0).abs() > 1e-9)
                .collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            (t.clone(), v)
        })
        .collect();
    TypeMatchups { defense, offense }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::tests::sample;
    use super::*;

    #[test]
    fn efetividade_amplificada_da_hunt() {
        let grass_poison = vec!["GRASS".to_string(), "POISON".to_string()];
        assert_eq!(type_effectiveness("FIRE", &grass_poison), 2.0);
        assert_eq!(amplify(2.0), 2.5);
        assert_eq!(amplify(4.0), 5.5);
        assert!((amplify(0.5) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(amplify(0.0), 0.0);
        assert_eq!(type_effectiveness("ELECTRIC", &["GROUND".to_string()]), 0.0);
    }

    #[test]
    fn ritmo_cresce_com_a_folga() {
        assert!(ritmo(0.1) < ritmo(1.0) && ritmo(1.0) < ritmo(3.0));
        assert!(ritmo(50.0) <= 1.0 && ritmo(50.0) > 0.95);
        assert!(golpes(3.0) < golpes(1.0) && golpes(3.0) >= 1.0);
        assert_eq!(ritmo(0.0), 0.0);
    }

    #[test]
    fn fogo_rende_mais_na_hunt_de_planta() {
        let cat = sample();
        let defs = Defenders::new(&cat);
        let charmeleon = cat.species_by_name("charmeleon").unwrap();
        let a = Attacker::standard(charmeleon, 40.0, Tms::None);
        let hunt = |slug: &str| cat.hunts.iter().position(|h| h.slug == slug).unwrap();
        let grass = sug_calc(&a, hunt("bulbasaur"), &defs).unwrap();
        let water = sug_calc(&a, hunt("squirtle"), &defs).unwrap();
        assert_eq!(grass.eff, 2.5);
        assert!(grass.folga > water.folga);
        assert!(grass.kills_h > water.kills_h);
        // cidade não tem selvagem
        assert!(sug_calc(&a, hunt("cerulean"), &defs).is_none());
    }

    #[test]
    fn tierlist_respeita_o_nivel() {
        let cat = sample();
        let all = tier_list(&cat, 0.0, false, false);
        assert!(all.iter().any(|r| r.species == "Charmeleon"));
        // no nível 20 o Charmeleon (evolui no 30) ainda não existe e a hunt de Squirtle (25) não conta
        let lv20 = tier_list(&cat, 20.0, false, false);
        assert!(lv20.iter().all(|r| r.species != "Charmeleon" && r.hunt.level <= 20.0));
        assert!(lv20.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn onde_cacar_planta_indica_fogo() {
        let cat = sample();
        let rows = hunts_by_type(&cat, "GRASS", 100.0, false, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].hunt.name, "Bulbasaur");
        assert_eq!(rows[0].species, "Charmeleon");
    }

    #[test]
    fn fraquezas_do_bulbasaur() {
        let m = matchups(&["GRASS".to_string(), "POISON".to_string()]);
        let fire = m.defense.iter().find(|(t, _)| t == "FIRE").unwrap().1;
        assert_eq!(fire, 2.5);
        assert!(m.defense.iter().all(|(t, _)| t != "NORMAL"));
    }
}

/// Roda com os arquivos reais do jogo: `PG_GAMEDATA=<pasta com creatures.json, items.json e map-markers> cargo test --release -- --ignored --nocapture`
#[cfg(test)]
mod real_data {
    use super::*;

    #[test]
    #[ignore]
    fn tierlist_com_dados_reais() {
        let dir = std::path::PathBuf::from(std::env::var("PG_GAMEDATA").expect("PG_GAMEDATA"));
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let cat = Catalog::build(&read("creatures.json"), &read("items.json"), &read("map-markers"), 0).unwrap();
        let with_species = cat.hunts.iter().filter(|h| !h.species.is_empty()).count();
        println!("{} espécies, {} hunts ({} com selvagem)", cat.species.len(), cat.hunts.len(), with_species);
        for (lv, tm, gold) in [(0.0, false, false), (100.0, false, false), (300.0, true, false), (300.0, false, true)] {
            let t = std::time::Instant::now();
            let rows = tier_list(&cat, lv, tm, gold);
            println!("\nnível {lv} tm {tm} gold {gold}: {} linhas em {:?}", rows.len(), t.elapsed());
            for r in rows.iter().take(6) {
                println!("  {:<22} {:?} {:>10.0}/h  {} Lv{} {} x{:.2} folga {:.2} {}", r.base, r.types, r.score, r.hunt.name, r.hunt.level, r.sug.move_name, r.sug.eff, r.sug.folga, r.sug.tm);
            }
        }
        let t = std::time::Instant::now();
        let rows = hunts_by_type(&cat, "FIRE", 150.0, false, false);
        println!("\nonde caçar FIRE lv150: {} hunts em {:?}", rows.len(), t.elapsed());
        for r in rows.iter().take(5) {
            println!("  {} Lv{} com {} {:.0} XP/h ({:.0} kills/h)", r.hunt.name, r.hunt.level, r.species, r.score, r.sug.kills_h);
        }
        assert!(with_species > 300);
    }
}
