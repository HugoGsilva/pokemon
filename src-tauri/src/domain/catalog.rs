//! Catálogo do jogo montado a partir dos arquivos públicos do site (`/game/creatures.json`, `/game/items.json`
//! e `/api/game/map-markers`): espécies com bases, tipos e golpes, e hunts com o selvagem de cada uma.
//! Mesmas regras de casamento do PokeGrid original (MIT).
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// `null` no JSON vira o valor padrão (o jogo manda `"type2": null` em algumas espécies)
fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize, Default)]
struct RawCreatures {
    #[serde(default, deserialize_with = "nullable")]
    creatures: Vec<RawCreature>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawCreature {
    #[serde(deserialize_with = "nullable")]
    poke_id: f64,
    #[serde(deserialize_with = "nullable")]
    name: String,
    #[serde(deserialize_with = "nullable")]
    type1: String,
    #[serde(deserialize_with = "nullable")]
    type2: String,
    #[serde(deserialize_with = "nullable")]
    base_hp: f64,
    #[serde(deserialize_with = "nullable")]
    base_atk: f64,
    #[serde(deserialize_with = "nullable")]
    base_def: f64,
    #[serde(deserialize_with = "nullable")]
    base_sp_atk: f64,
    #[serde(deserialize_with = "nullable")]
    base_sp_def: f64,
    #[serde(deserialize_with = "nullable")]
    base_speed: f64,
    #[serde(deserialize_with = "nullable")]
    hunt_level: f64,
    #[serde(deserialize_with = "nullable")]
    experience: f64,
    #[serde(deserialize_with = "nullable")]
    area: String,
    #[serde(deserialize_with = "nullable")]
    orre_xp_mul: f64,
    #[serde(deserialize_with = "nullable")]
    evolves_to_id: f64,
    #[serde(deserialize_with = "nullable")]
    evolve_level: f64,
    #[serde(deserialize_with = "nullable")]
    loot: Vec<RawLoot>,
    #[serde(deserialize_with = "nullable")]
    attacks: Vec<RawAttack>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawLoot {
    #[serde(deserialize_with = "nullable")]
    name: String,
    #[serde(deserialize_with = "nullable")]
    chance: f64,
    #[serde(deserialize_with = "nullable")]
    min_count: f64,
    #[serde(deserialize_with = "nullable")]
    max_count: f64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawAttack {
    #[serde(deserialize_with = "nullable")]
    name: String,
    #[serde(deserialize_with = "nullable")]
    power: f64,
    #[serde(rename = "type", deserialize_with = "nullable")]
    ty: String,
    #[serde(deserialize_with = "nullable")]
    category: String,
    #[serde(deserialize_with = "nullable")]
    cooldown_ms: f64,
    #[serde(deserialize_with = "nullable")]
    learn_level: f64,
    #[serde(deserialize_with = "nullable")]
    tm: String,
}

#[derive(Deserialize, Default)]
struct RawItems {
    #[serde(default, deserialize_with = "nullable")]
    items: Vec<RawItem>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawItem {
    #[serde(deserialize_with = "nullable")]
    id: f64,
    #[serde(deserialize_with = "nullable")]
    name: String,
    #[serde(deserialize_with = "nullable")]
    npc_price: f64,
}

#[derive(Deserialize, Default)]
struct RawMarkers {
    #[serde(default, deserialize_with = "nullable")]
    hunts: Vec<RawMarker>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawMarker {
    #[serde(deserialize_with = "nullable")]
    slug: String,
    #[serde(deserialize_with = "nullable")]
    name: String,
    #[serde(deserialize_with = "nullable")]
    level: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub name: String,
    pub power: f64,
    pub ty: String,
    pub physical: bool,
    pub cooldown_ms: f64,
    pub learn_level: f64,
    /// Golpe de disco (TM): não substitui o golpe normal
    pub tm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Species {
    pub id: u32,
    pub name: String,
    pub types: Vec<String>,
    pub bases: [f64; 6],
    pub hunt_level: f64,
    /// Menor nível em que dá para ter a espécie (caçando ou evoluindo)
    pub min_level: f64,
    pub is_final: bool,
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunt {
    pub slug: String,
    pub name: String,
    pub level: f64,
    /// kanto | orre | outland | nightmare | "" (área da espécie selvagem)
    pub area: String,
    /// Chave (nome em minúsculas) da espécie selvagem; vazio em cidades
    pub species: String,
    /// Id da espécie selvagem, ou da base quando é uma forma do jogo (para o sprite; 0 em cidades)
    #[serde(default)]
    pub species_id: u32,
    pub types: Vec<String>,
    pub xp: f64,
    /// Loot esperado por kill a preço de NPC
    pub gold_per_kill: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    /// Unix (s) do download
    pub fetched_at: u64,
    pub species: Vec<Species>,
    pub hunts: Vec<Hunt>,
    /// Preço de NPC por id de item (o loot que o jogo manda em cada abate vem pelo id). Cache antigo vem sem
    #[serde(default)]
    pub item_prices: HashMap<u32, f64>,
    #[serde(skip)]
    by_key: HashMap<String, usize>,
}

/// Maior número da Pokédex nacional (formas próprias do jogo têm ids de 10000 em diante)
const NATIONAL_DEX_MAX: u32 = 1025;

/// Exceções do jogo: espécies que evoluem mas já contam como estágio final (aprendem TM)
const FINAL_EXCEPTIONS: [u32; 9] = [123, 112, 114, 125, 126, 356, 375, 372, 200];

fn limpa(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut depth_sq = 0;
    let mut depth_par = 0;
    for ch in lower.chars() {
        match ch {
            '[' => depth_sq += 1,
            ']' if depth_sq > 0 => depth_sq -= 1,
            '(' => depth_par += 1,
            ')' if depth_par > 0 => depth_par -= 1,
            _ if depth_sq > 0 || depth_par > 0 => {}
            '_' | '-' => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn chave(s: &str) -> String {
    limpa(s)
        .replace("female", "fe")
        .replace("male", "ma")
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// Normaliza o tipo como o jogo escreve (`FIRE`); vazio fica de fora
fn types_of(c: &RawCreature) -> Vec<String> {
    [&c.type1, &c.type2]
        .into_iter()
        .map(|t| t.trim().to_uppercase())
        .filter(|t| !t.is_empty())
        .collect()
}

fn area_of(c: &RawCreature) -> String {
    if !c.area.is_empty() {
        c.area.to_lowercase()
    } else if (10000.0..11000.0).contains(&c.poke_id) {
        "outland".into()
    } else {
        String::new()
    }
}

/// Espécie repetida (formas de Outland/Mega com o mesmo nome): a sem área e id alto não substitui a anterior
fn keeps_previous(exists: bool, c: &RawCreature) -> bool {
    exists && c.area.is_empty() && c.poke_id >= 10000.0
}

impl Catalog {
    pub fn build(creatures_json: &str, items_json: &str, markers_json: &str, fetched_at: u64) -> Result<Self, String> {
        let creatures: RawCreatures =
            serde_json::from_str(creatures_json).map_err(|e| format!("creatures.json: {}", e))?;
        let items: RawItems = serde_json::from_str(items_json).unwrap_or_default();
        let markers: RawMarkers =
            serde_json::from_str(markers_json).map_err(|e| format!("map-markers: {}", e))?;
        let creatures: Vec<RawCreature> =
            creatures.creatures.into_iter().filter(|c| !c.name.trim().is_empty()).collect();
        if creatures.is_empty() {
            return Err("creatures.json veio sem espécies".into());
        }

        // Preço de NPC por nome (exato e em minúsculas)
        let mut price: HashMap<&str, f64> = HashMap::new();
        let mut price_lower: HashMap<String, f64> = HashMap::new();
        for it in &items.items {
            price.insert(it.name.as_str(), it.npc_price);
            price_lower.entry(it.name.to_lowercase()).or_insert(it.npc_price);
        }
        let gold_of = |c: &RawCreature| -> f64 {
            c.loot
                .iter()
                .filter(|l| l.chance > 0.0)
                .map(|l| {
                    let p = price
                        .get(l.name.as_str())
                        .copied()
                        .or_else(|| price_lower.get(&l.name.to_lowercase()).copied())
                        .unwrap_or(0.0);
                    let min = if l.min_count > 0.0 { l.min_count } else { 1.0 };
                    let max = if l.max_count > 0.0 { l.max_count } else { min };
                    l.chance / 1e5 * (min + max) / 2.0 * p
                })
                .sum()
        };

        // Selvagem de cada hunt, por chave normalizada do nome
        let mut by_chave: HashMap<String, usize> = HashMap::new();
        for (i, c) in creatures.iter().enumerate() {
            let k = chave(&c.name);
            if !keeps_previous(by_chave.contains_key(&k), c) {
                by_chave.insert(k, i);
            }
        }
        let acha = |nm: &str| -> Option<usize> {
            let k = chave(nm);
            if let Some(&i) = by_chave.get(&k) {
                return Some(i);
            }
            let l = limpa(nm);
            let parts: Vec<&str> = l.split(" e ").flat_map(|p| p.split(" and ")).collect();
            if parts.len() > 1 {
                if let Some(&i) = by_chave.get(&chave(parts[0])) {
                    return Some(i);
                }
            }
            let words: Vec<&str> = l.split(' ').collect();
            (0..words.len())
                .find_map(|i| by_chave.get(&chave(&words[i..].join(" "))).copied())
                .or_else(|| words.iter().find_map(|w| by_chave.get(&chave(w)).copied()))
        };

        // Menor nível para ter cada id: huntLevel, propagado pelas evoluções
        let mut min_by_id: HashMap<u32, f64> =
            creatures.iter().map(|c| (c.poke_id as u32, c.hunt_level)).collect();
        for _ in 0..4 {
            for c in &creatures {
                let e = c.evolves_to_id as u32;
                if e == 0 {
                    continue;
                }
                let Some(&cur) = min_by_id.get(&e) else { continue };
                let v = min_by_id.get(&(c.poke_id as u32)).copied().unwrap_or(0.0).max(c.evolve_level);
                if v < cur {
                    min_by_id.insert(e, v);
                }
            }
        }

        // Espécies por nome (mesma regra de repetidos); o id fica o menor
        let mut species: Vec<Species> = Vec::new();
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let name_by_id: HashMap<u32, String> =
            creatures.iter().map(|c| (c.poke_id as u32, c.name.to_lowercase())).collect();
        for c in &creatures {
            let key = c.name.to_lowercase().trim().to_string();
            let prev = by_key.get(&key).copied();
            if keeps_previous(prev.is_some(), c) {
                continue;
            }
            let id = c.poke_id as u32;
            let is_final = c.evolves_to_id == 0.0 || FINAL_EXCEPTIONS.contains(&id);
            let moves = c
                .attacks
                .iter()
                .take(24)
                .map(|a| Move {
                    name: a.name.chars().take(24).collect(),
                    power: a.power,
                    ty: a.ty.to_uppercase(),
                    physical: a.category.eq_ignore_ascii_case("PHYSICAL"),
                    cooldown_ms: a.cooldown_ms,
                    learn_level: a.learn_level,
                    tm: a.tm.to_uppercase().chars().take(12).collect(),
                })
                .collect();
            let sp = Species {
                id: prev.map(|p| species[p].id.min(id)).unwrap_or(id),
                name: c.name.trim().to_string(),
                types: types_of(c),
                bases: [c.base_hp, c.base_atk, c.base_def, c.base_sp_atk, c.base_sp_def, c.base_speed],
                hunt_level: c.hunt_level,
                min_level: min_by_id.get(&id).copied().unwrap_or(c.hunt_level),
                is_final,
                moves,
            };
            match prev {
                Some(p) => species[p] = sp,
                None => {
                    by_key.insert(key, species.len());
                    species.push(sp);
                }
            }
        }

        // Nível mínimo pelas hunts do mapa (onde a espécie é caçada), propagado pelas evoluções
        let mut min_by_name: HashMap<String, f64> = HashMap::new();
        for m in &markers.hunts {
            let nm = if m.name.is_empty() { &m.slug } else { &m.name };
            if let Some(i) = acha(nm) {
                let k = creatures[i].name.to_lowercase().trim().to_string();
                if m.level > 0.0 && min_by_name.get(&k).map_or(true, |&v| m.level < v) {
                    min_by_name.insert(k, m.level);
                }
            }
        }
        for _ in 0..4 {
            for c in &creatures {
                let e = c.evolves_to_id as u32;
                let Some(pa) = name_by_id.get(&e).filter(|_| e != 0) else { continue };
                let Some(&de) = min_by_name.get(&c.name.to_lowercase()) else { continue };
                let v = de.max(c.evolve_level);
                if min_by_name.get(pa).map_or(true, |&cur| v < cur) {
                    min_by_name.insert(pa.clone(), v);
                }
            }
        }
        for (k, &i) in &by_key {
            if let Some(&v) = min_by_name.get(k) {
                species[i].min_level = v;
            }
        }

        // Id para o sprite: formas do jogo (Furious Scyther, Nightmare Alolan Raichu) usam o da espécie-base
        let dex_id = |name: &str| -> u32 {
            let words: Vec<&str> = name.trim().split(' ').collect();
            let ids = (0..words.len())
                .filter_map(|k| by_key.get(&words[k..].join(" ").to_lowercase()).map(|&i| species[i].id));
            let mut first = 0;
            for id in ids {
                if id <= NATIONAL_DEX_MAX {
                    return id;
                }
                if first == 0 {
                    first = id;
                }
            }
            first
        };
        let hunts = markers
            .hunts
            .iter()
            .filter_map(|m| {
                let nm = if m.name.is_empty() { &m.slug } else { &m.name };
                if nm.is_empty() {
                    return None;
                }
                let c = acha(nm).map(|i| &creatures[i]);
                let orre_mul = c.filter(|c| c.area == "orre" && c.orre_xp_mul > 0.0).map_or(1.0, |c| c.orre_xp_mul);
                Some(Hunt {
                    slug: m.slug.clone(),
                    name: nm.replace(['_', '-'], " ").trim().to_string(),
                    level: if m.level > 0.0 { m.level } else { c.map_or(0.0, |c| c.hunt_level) },
                    area: c.map(area_of).unwrap_or_default(),
                    species: c.map(|c| c.name.to_lowercase().trim().to_string()).unwrap_or_default(),
                    species_id: c.map_or(0, |c| dex_id(&c.name)),
                    types: c.map(types_of).unwrap_or_default(),
                    xp: c.map_or(0.0, |c| (c.experience * orre_mul).round()),
                    gold_per_kill: c.map_or(0.0, |c| gold_of(c).round()),
                })
            })
            .collect();

        let item_prices = items
            .items
            .iter()
            .filter(|it| it.id > 0.0 && it.npc_price > 0.0)
            .map(|it| (it.id as u32, it.npc_price))
            .collect();

        Ok(Catalog { fetched_at, species, hunts, item_prices, by_key })
    }

    /// Reconstrói o índice por nome depois de ler o cache do disco
    pub fn reindex(&mut self) {
        self.by_key = self
            .species
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.to_lowercase(), i))
            .collect();
    }

    pub fn species_by_name(&self, name: &str) -> Option<&Species> {
        self.by_key.get(&name.trim().to_lowercase()).map(|&i| &self.species[i])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Catálogo mínimo com o formato real dos arquivos do jogo
    pub fn sample() -> Catalog {
        let creatures = r#"{"creatures":[
          {"pokeId":4,"name":"Charmander","type1":"FIRE","type2":null,"baseHp":39,"baseAtk":52,"baseDef":43,"baseSpAtk":60,"baseSpDef":50,"baseSpeed":65,"huntLevel":20,"experience":200,"evolvesToId":5,"evolveLevel":30,
           "loot":[{"name":"Fire Stone","chance":50000,"minCount":1,"maxCount":3}],
           "attacks":[{"name":"Ember","power":40,"type":"FIRE","category":"SPECIAL","cooldownMs":2000,"learnLevel":1},
                      {"name":"Scratch","power":40,"type":"NORMAL","category":"PHYSICAL","cooldownMs":2000,"learnLevel":1}]},
          {"pokeId":5,"name":"Charmeleon","type1":"FIRE","baseHp":58,"baseAtk":64,"baseDef":58,"baseSpAtk":80,"baseSpDef":65,"baseSpeed":80,"huntLevel":60,"experience":500,"evolvesToId":0,"evolveLevel":0,
           "attacks":[{"name":"Flamethrower","power":90,"type":"FIRE","category":"SPECIAL","cooldownMs":2000,"learnLevel":1}]},
          {"pokeId":1,"name":"Bulbasaur","type1":"GRASS","type2":"POISON","baseHp":45,"baseAtk":49,"baseDef":49,"baseSpAtk":65,"baseSpDef":65,"baseSpeed":45,"huntLevel":20,"experience":248,"evolvesToId":0,
           "attacks":[{"name":"Vine Whip","power":45,"type":"GRASS","category":"PHYSICAL","cooldownMs":2000,"learnLevel":1}]},
          {"pokeId":7,"name":"Squirtle","type1":"WATER","baseHp":44,"baseAtk":48,"baseDef":65,"baseSpAtk":50,"baseSpDef":64,"baseSpeed":43,"huntLevel":20,"experience":220,"evolvesToId":0,
           "attacks":[{"name":"Water Gun","power":40,"type":"WATER","category":"SPECIAL","cooldownMs":2000,"learnLevel":1}]}
        ]}"#;
        let items = r#"{"items":[{"id":12,"name":"Fire Stone","npcPrice":100},{"id":13,"name":"Sem preço","npcPrice":null}]}"#;
        let markers = r#"{"hunts":[
          {"slug":"cerulean","name":"Cerulean","level":0},
          {"slug":"bulbasaur","name":"Bulbasaur","level":20},
          {"slug":"charmander","name":"Charmander","level":15},
          {"slug":"squirtle","name":"Squirtle","level":25}
        ]}"#;
        Catalog::build(creatures, items, markers, 0).unwrap()
    }

    #[test]
    fn monta_especies_e_hunts() {
        let cat = sample();
        let charmander = cat.species_by_name("charmander").unwrap();
        assert_eq!(charmander.types, vec!["FIRE"]);
        assert!(!charmander.is_final);
        // caçado no mapa a partir do 15; o Charmeleon vem por evolução no 30
        assert_eq!(charmander.min_level, 15.0);
        assert_eq!(cat.species_by_name("Charmeleon").unwrap().min_level, 30.0);

        let hunt = cat.hunts.iter().find(|h| h.slug == "charmander").unwrap();
        assert_eq!(hunt.species, "charmander");
        assert_eq!(hunt.species_id, 4);
        // 0,5 de chance x média de 2 x 100
        assert_eq!(hunt.gold_per_kill, 100.0);
        let city = cat.hunts.iter().find(|h| h.slug == "cerulean").unwrap();
        assert!(city.species.is_empty());

        // Preço por id para o loot do painel do Modo Eco; item sem preço fica de fora
        assert_eq!(cat.item_prices.get(&12), Some(&100.0));
        assert!(!cat.item_prices.contains_key(&13));
    }

    #[test]
    fn normaliza_nomes_de_hunt() {
        assert_eq!(chave("Nidoran (Female)"), "nidoran");
        assert_eq!(chave("Mr. Mime"), "mrmime");
        assert_eq!(limpa("Dark_Crobat [Boss]"), "dark crobat");
    }
}
