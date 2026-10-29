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

pub const EXPONENTS: [f64; 6] = [0.95, 0.80, 0.80, 0.80, 0.80, 0.95];

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
}
