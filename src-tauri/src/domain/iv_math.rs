//! Fórmula de atributos do Poke Idle World e leitura de IV a partir dos atributos que o jogo mostra.
//! Portado do `iv-math.js` do PokeGrid original (MIT), com os mesmos casos de teste.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatValues {
    pub hp: u32,
    pub atk: u32,
    pub def: u32,
    pub spa: u32,
    pub spd: u32,
    pub speed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseStats {
    pub hp: u32,
    pub atk: u32,
    pub def: u32,
    pub spa: u32,
    pub spd: u32,
    pub speed: u32,
}

/// Ordem dos atributos em todos os arrays: HP, Atk, Def, SpA, SpD, Speed
pub const EXPONENTS: [f64; 6] = [0.95, 0.80, 0.80, 0.80, 0.80, 0.95];
pub const IV_MAX: u32 = 32;
/// IV total "mediano" usado quando não se sabe o IV (selvagens e atacantes da tierlist)
pub const IV_TOTAL_MEDIO: f64 = 96.0;

/// Calcula um atributo individual projetado pela fórmula do Poke Idle World
pub fn calc_stat(base: u32, growth: u32, level: u32, quality: f64, exp: f64) -> u32 {
    let base_calc = (base + 2 * growth) as f64;
    let lvl_ratio = (level as f64) / 100.0;
    let qual_factor = quality.powf(exp);
    (base_calc * lvl_ratio * qual_factor).round() as u32
}

/// Encontra os possíveis valores de growth (IV de 1 a 32) para um atributo específico
pub fn solve_growth_candidates(base: u32, target_stat: u32, level: u32, quality: f64, exp: f64) -> Vec<u32> {
    let mut candidates = Vec::new();
    for g in 1..=32 {
        if calc_stat(base, g, level, quality, exp) == target_stat {
            candidates.push(g);
        }
    }
    candidates
}

/// Atributo `i` (0..6) projetado; `None` com nível ou qualidade inválidos
pub fn project_stat(base: f64, growth: f64, level: f64, quality: f64, i: usize) -> Option<u32> {
    if !(level > 0.0) || !(quality > 0.0) || !base.is_finite() || !growth.is_finite() {
        return None;
    }
    let v = (base + 2.0 * growth) * (level / 100.0) * quality.powf(EXPONENTS[i]);
    Some(v.round().max(0.0) as u32)
}

/// O que se sabe do pokémon: atributos lidos no jogo e/ou IVs individuais
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IvInput {
    pub bases: [f64; 6],
    pub level: f64,
    pub quality: f64,
    /// Casas decimais da qualidade digitada (o jogo mostra 2); mais casas = mais precisão
    pub quality_decimals: u32,
    pub iv_total: Option<f64>,
    pub power: Option<f64>,
    pub observed: Option<[Option<f64>; 6]>,
    pub ivs: Option<[Option<f64>; 6]>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IvResult {
    pub values: [u32; 6],
    pub ranges: [Option<(u32, u32)>; 6],
    /// individual | observed-exact | observed-range | total-scenario | unknown
    pub source: &'static str,
    pub exact: bool,
    /// Qualidade de verdade achada pelos atributos (quando houve leitura)
    pub quality: Option<f64>,
}

/// Conjunto de valores 0..=192 (soma de 6 IVs) como bitmap simples
type Sums = [bool; 6 * IV_MAX as usize + 1];

fn possible_sums(groups: &[Vec<u32>]) -> Sums {
    let mut sums = [false; 6 * IV_MAX as usize + 1];
    sums[0] = true;
    for group in groups {
        let mut next = [false; 6 * IV_MAX as usize + 1];
        for (s, on) in sums.iter().enumerate() {
            if !on {
                continue;
            }
            for &v in group {
                if let Some(slot) = next.get_mut(s + v as usize) {
                    *slot = true;
                }
            }
        }
        sums = next;
    }
    sums
}

fn constrain_by_total(groups: &[Vec<u32>], wanted: i64) -> Vec<Vec<u32>> {
    if groups.iter().any(|g| g.is_empty()) {
        return groups.to_vec();
    }
    (0..groups.len())
        .map(|i| {
            let before = possible_sums(&groups[..i]);
            let after = possible_sums(&groups[i + 1..]);
            let allowed: Vec<u32> = groups[i]
                .iter()
                .copied()
                .filter(|&v| {
                    before.iter().enumerate().any(|(l, &on)| {
                        let r = wanted - l as i64 - v as i64;
                        on && r >= 0 && after.get(r as usize).copied().unwrap_or(false)
                    })
                })
                .collect();
            if allowed.is_empty() {
                groups[i].clone()
            } else {
                allowed
            }
        })
        .collect()
}

fn infer_candidates(observed: f64, base: f64, level: f64, quality: f64, i: usize) -> Vec<u32> {
    (1..=IV_MAX)
        .filter(|&g| project_stat(base, g as f64, level, quality, i).map(|v| v as f64) == Some(observed))
        .collect()
}

/// Só o IV total: distribui por igual (cenário), com a faixa que cada atributo pode ter
pub fn scenario_from_total(total: Option<f64>) -> IvResult {
    let Some(total) = total.filter(|t| t.is_finite() && *t > 0.0) else {
        return IvResult { values: [0; 6], ranges: [None; 6], source: "unknown", exact: false, quality: None };
    };
    let wanted = total.round().clamp(6.0, (6 * IV_MAX) as f64) as u32;
    let mut values = [1u32; 6];
    let mut remaining = wanted - 6;
    let mut cursor = 0;
    while remaining > 0 {
        let i = cursor % 6;
        cursor += 1;
        if values[i] < IV_MAX {
            values[i] += 1;
            remaining -= 1;
        }
    }
    let min = wanted.saturating_sub(5 * IV_MAX).max(1);
    let max = (wanted - 5).min(IV_MAX);
    IvResult { values, ranges: [Some((min, max)); 6], source: "total-scenario", exact: false, quality: None }
}

/// A qualidade que o jogo mostra vem com 2 casas: a de verdade está a até meia casa dela. O poder do jogo
/// (round da soma dos atributos x qualidade) aperta a janela quando vem junto.
fn quality_window(quality: f64, decimals: u32, power: Option<f64>, observed: &[f64; 6]) -> (f64, f64) {
    let half = if decimals > 2 { 0.5 * 10f64.powi(-(decimals.min(9) as i32)) } else { 0.005 };
    let (mut lo, mut hi) = (quality - half, quality + half);
    let sum: f64 = observed.iter().sum();
    if let Some(p) = power.filter(|p| *p > 0.0) {
        if sum > 0.0 {
            let plo = (p - 0.5) / sum;
            let phi = (p + 0.5) / sum;
            if plo <= hi && phi >= lo {
                lo = lo.max(plo);
                hi = hi.min(phi);
            }
        }
    }
    (lo.max(1e-6), hi)
}

/// Um q no meio de cada trecho da janela entre os pontos em que algum atributo muda de valor
fn quality_probes(lo: f64, hi: f64, observed: &[f64; 6], bases: &[f64; 6], level: f64) -> Vec<f64> {
    let mut points = vec![lo, hi];
    for i in 0..6 {
        for g in 1..=IV_MAX {
            let x = (bases[i] + 2.0 * g as f64) * level / 100.0;
            if !(x > 0.0) {
                continue;
            }
            for v in [observed[i] - 0.5, observed[i] + 0.5] {
                if v <= 0.0 {
                    continue;
                }
                let q = (v / x).powf(1.0 / EXPONENTS[i]);
                if q > lo && q < hi {
                    points.push(q);
                }
            }
        }
    }
    points.sort_by(|a, b| a.total_cmp(b));
    let probes: Vec<f64> = points
        .windows(2)
        .filter(|w| w[1] - w[0] > 1e-12)
        .map(|w| (w[0] + w[1]) / 2.0)
        .collect();
    if probes.is_empty() {
        vec![(lo + hi) / 2.0]
    } else {
        probes
    }
}

/// IVs por atributo: individuais se informados; senão inferidos dos atributos lidos; senão o cenário do IV total
pub fn normalize_ivs(input: &IvInput) -> IvResult {
    if let Some(ivs) = input.ivs {
        if ivs.iter().all(|v| v.is_some_and(f64::is_finite)) {
            let values = ivs.map(|v| v.unwrap().clamp(0.0, IV_MAX as f64) as u32);
            return IvResult {
                values,
                ranges: values.map(|v| Some((v, v))),
                source: "individual",
                exact: true,
                quality: None,
            };
        }
    }

    let observed = match input.observed {
        Some(o) if o.iter().all(|v| v.is_some_and(f64::is_finite)) => o.map(Option::unwrap),
        _ => return scenario_from_total(input.iv_total),
    };
    let (level, quality, bases) = (input.level, input.quality, &input.bases);
    if !(level > 0.0) || !(quality > 0.0) {
        return scenario_from_total(input.iv_total);
    }

    let (lo, hi) = quality_window(quality, input.quality_decimals, input.power, &observed);
    let wanted = input.iv_total.filter(|t| t.is_finite()).map(|t| t.round() as i64);

    let collect = |probes: &[f64], use_total: bool| -> Option<([Vec<u32>; 6], f64)> {
        let mut union: [[bool; IV_MAX as usize + 1]; 6] = [[false; IV_MAX as usize + 1]; 6];
        let (mut q_min, mut q_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for &q in probes {
            let groups: Vec<Vec<u32>> =
                (0..6).map(|i| infer_candidates(observed[i], bases[i], level, q, i)).collect();
            if groups.iter().any(|g| g.is_empty()) {
                continue;
            }
            let groups = match (use_total, wanted) {
                (true, Some(w)) => {
                    let ok = usize::try_from(w).ok().and_then(|w| possible_sums(&groups).get(w).copied());
                    if ok != Some(true) {
                        continue;
                    }
                    constrain_by_total(&groups, w)
                }
                _ => groups,
            };
            for (i, g) in groups.iter().enumerate() {
                for &v in g {
                    union[i][v as usize] = true;
                }
            }
            q_min = q_min.min(q);
            q_max = q_max.max(q);
        }
        if q_max < q_min {
            return None;
        }
        let sets = std::array::from_fn(|i| {
            (0..=IV_MAX).filter(|&v| union[i][v as usize]).collect::<Vec<u32>>()
        });
        Some((sets, (q_min + q_max) / 2.0))
    };

    let window = quality_probes(lo, hi, &observed, bases, level);
    // O jogo sorteia a qualidade com 3 casas: esses valores vêm primeiro; sem nenhum que feche, a janela contínua
    let grade: Vec<f64> = ((lo * 1000.0 - 1e-9).ceil() as i64..=(hi * 1000.0 + 1e-9).floor() as i64)
        .map(|k| k as f64 / 1000.0)
        .collect();
    let tenta = |use_total: bool| {
        (!grade.is_empty())
            .then(|| collect(&grade, use_total))
            .flatten()
            .or_else(|| collect(&window, use_total))
    };
    let found = wanted.and_then(|_| tenta(true)).or_else(|| tenta(false));
    let Some((groups, found_quality)) = found else {
        return scenario_from_total(input.iv_total);
    };

    let mut values = [0u32; 6];
    let mut ranges = [None; 6];
    for i in 0..6 {
        let (min, max) = (groups[i][0], *groups[i].last().unwrap());
        ranges[i] = Some((min, max));
        values[i] = if groups[i].len() == 1 { min } else { ((min + max) as f64 / 2.0).round() as u32 };
    }
    let exact = groups.iter().all(|g| g.len() == 1);
    IvResult {
        values,
        ranges,
        source: if exact { "observed-exact" } else { "observed-range" },
        exact,
        quality: Some(found_quality),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub stats: [u32; 6],
    pub power: u32,
    /// Qualidade usada na conta (a de verdade, quando a leitura achou)
    pub quality: f64,
}

/// Atributos e poder em `level`, com os IVs de `iv`
pub fn project(bases: &[f64; 6], iv: &IvResult, fallback_quality: f64, level: f64) -> Option<Projection> {
    let quality = iv.quality.unwrap_or(fallback_quality).max(0.01);
    let mut stats = [0u32; 6];
    for i in 0..6 {
        stats[i] = project_stat(bases[i], iv.values[i] as f64, level, quality, i)?;
    }
    let power = (stats.iter().sum::<u32>() as f64 * quality).round() as u32;
    Some(Projection { stats, power, quality })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_stat_formula() {
        // Exemplo: base 100, growth 31, level 100, quality 1.0, exp 0.80 -> (100 + 62) * 1 * 1 = 162
        let stat = calc_stat(100, 31, 100, 1.0, 0.80);
        assert_eq!(stat, 162);
    }

    #[test]
    fn test_solve_growth() {
        let candidates = solve_growth_candidates(100, 162, 100, 1.0, 0.80);
        assert!(candidates.contains(&31));
    }

    const GYARADOS: [f64; 6] = [95.0, 125.0, 79.0, 60.0, 100.0, 81.0];
    const GYARADOS_IVS: [u32; 6] = [31, 30, 28, 27, 30, 31];

    fn gyarados() -> IvInput {
        IvInput {
            bases: GYARADOS,
            level: 138.0,
            quality: 1.53,
            quality_decimals: 2,
            iv_total: Some(177.0),
            ivs: Some(GYARADOS_IVS.map(|v| Some(v as f64))),
            ..Default::default()
        }
    }

    #[test]
    fn projeta_gyarados_no_nivel_atual_e_no_500() {
        let input = gyarados();
        let iv = normalize_ivs(&input);
        let now = project(&GYARADOS, &iv, input.quality, 138.0).unwrap();
        assert_eq!(now.stats, [325, 359, 262, 221, 310, 296]);
        assert_eq!(now.power, 2713);
        let lv500 = project(&GYARADOS, &iv, input.quality, 500.0).unwrap();
        assert_eq!(lv500.stats, [1176, 1300, 949, 801, 1124, 1071]);
        assert_eq!(lv500.power, 9824);
    }

    #[test]
    fn infere_ivs_exatos_pelos_atributos() {
        let input = IvInput {
            ivs: None,
            observed: Some([325.0, 359.0, 262.0, 221.0, 310.0, 296.0].map(Some)),
            ..gyarados()
        };
        let iv = normalize_ivs(&input);
        assert_eq!(iv.source, "observed-exact");
        assert_eq!(iv.values, GYARADOS_IVS);
    }

    #[test]
    fn so_o_total_vira_cenario() {
        let input = IvInput { ivs: None, ..gyarados() };
        let iv = normalize_ivs(&input);
        assert_eq!(iv.source, "total-scenario");
        assert!(!iv.exact);
        assert_eq!(iv.ranges[0], Some((17, 32)));
    }

    fn tyranitar() -> IvInput {
        IvInput {
            bases: [100.0, 134.0, 110.0, 95.0, 100.0, 61.0],
            level: 149.0,
            quality: 1.71,
            quality_decimals: 2,
            iv_total: Some(140.0),
            power: Some(3513.0),
            observed: Some([386.0, 452.0, 370.0, 240.0, 361.0, 250.0].map(Some)),
            ivs: None,
        }
    }
    const TYRANITAR_IVS: [u32; 6] = [28, 32, 26, 5, 29, 20];

    #[test]
    fn qualidade_arredondada_usa_a_janela_e_o_poder() {
        let input = tyranitar();
        let iv = normalize_ivs(&input);
        assert_eq!(iv.source, "observed-exact");
        assert_eq!(iv.values, TYRANITAR_IVS);
        let q = iv.quality.unwrap();
        assert!(q > 3512.5 / 2059.0 && q < 3513.5 / 2059.0, "qualidade {q}");
        assert_eq!(project(&input.bases, &iv, input.quality, 149.0).unwrap().power, 3513);

        let sem_poder = normalize_ivs(&IvInput { power: None, ..tyranitar() });
        assert_eq!(sem_poder.values, TYRANITAR_IVS);
        assert!(sem_poder.exact);

        let digitada = normalize_ivs(&IvInput { power: None, quality: 1.7062, quality_decimals: 4, ..tyranitar() });
        assert_eq!(digitada.values, TYRANITAR_IVS);
    }

    #[test]
    fn qualidade_de_3_casas_nunca_da_exato_errado() {
        let input = IvInput {
            bases: [150.0, 160.0, 100.0, 95.0, 65.0, 100.0],
            level: 20.0,
            quality: 1.43,
            quality_decimals: 2,
            iv_total: Some(123.0),
            power: Some(353.0),
            observed: Some([55.0, 54.0, 41.0, 42.0, 20.0, 36.0].map(Some)),
            ivs: None,
        };
        let verdade = [24, 21, 27, 31, 6, 14];
        let iv = normalize_ivs(&input);
        for i in 0..6 {
            let (lo, hi) = iv.ranges[i].unwrap();
            assert!(lo <= verdade[i] && verdade[i] <= hi, "atributo {i}: {:?}", iv.ranges);
        }
        assert!(!iv.exact || iv.values == verdade);
    }

    #[test]
    fn total_96_distribui_16_em_cada() {
        assert_eq!(scenario_from_total(Some(96.0)).values, [16; 6]);
    }
}
