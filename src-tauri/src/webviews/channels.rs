use std::fs;
use std::path::Path;

/// Canais oficiais do Poke Idle World na Twitch (lista do jogo em 29/10/2026). Só contam para o
/// bônus de live os canais oficiais com o selo "⚡ BONUS ON"; a ordem define a prioridade.
pub const DEFAULT_CHANNELS: &[&str] = &[
    "mariotv18s",
    "daldreak",
    "macabeel",
    "noobdemaisoficial",
    "narkesh",
    "xuukz",
    "raku_m",
    "stigalzera",
    "lugontv",
    "niusaur",
    "efac13",
    "cabelo182",
    "benksbr",
    "pitersonpiu",
    "teuz2k",
    "berudo1997",
    "belgatcg",
    "jrdferri",
    "afroplays22",
    "molejaooo",
    "galyleugaliley",
    "opiadogaita",
];

/// Aceita "Canal", "@canal" ou "https://www.twitch.tv/canal"; descarta inválidos e repetidos, mantendo a ordem
pub fn normalize(input: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input {
        let mut name = raw.trim().to_lowercase();
        if let Some(pos) = name.find("twitch.tv/") {
            name = name[pos + "twitch.tv/".len()..].to_string();
        }
        let name = name
            .trim_start_matches('@')
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            .to_string();

        // Logins da Twitch: 1 a 25 caracteres, letras, números e "_"
        let valid = (1..=25).contains(&name.len())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if valid && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Lista salva pelo usuário; sem arquivo (ou arquivo inválido) usa a lista padrão
pub fn load(path: &Path) -> Vec<String> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .map(|list| normalize(&list))
        .unwrap_or_else(|| DEFAULT_CHANNELS.iter().map(|c| c.to_string()).collect())
}

pub fn save(path: &Path, channels: &[String]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Erro ao criar pasta de dados: {}", e))?;
    }
    let json = serde_json::to_vec_pretty(channels).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_urls_and_handles() {
        let input = vec![
            " LugonTV ".to_string(),
            "@Raku_m".to_string(),
            "https://www.twitch.tv/Efac13/videos?x=1".to_string(),
            "lugontv".to_string(),
            "nome com espaço".to_string(),
            "".to_string(),
        ];
        assert_eq!(normalize(&input), vec!["lugontv", "raku_m", "efac13"]);
    }
}
