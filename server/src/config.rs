//! Configuração por variáveis de ambiente (ver `server/README.md`)
use std::path::PathBuf;

pub struct Config {
    pub bind: String,
    pub user: String,
    /// Senha do login (Basic auth). Sem ela o servidor não sobe, a não ser com `PG_NO_AUTH=1`
    pub password: Option<String>,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub chrome: Option<String>,
    pub game_url: String,
    pub accounts: usize,
    pub names: Vec<String>,
    pub width: u32,
    pub height: u32,
    /// Quadros por segundo na grade e com a conta ampliada
    pub fps_grid: f64,
    pub fps_focus: f64,
    pub jpeg_quality: u32,
    /// Teto de RAM de uma conta (árvore de processos do Chromium dela, PSS)
    pub mem_account_mb: u64,
    /// Teto de RAM das contas somadas
    pub mem_total_mb: u64,
    /// Limite do heap de JavaScript por página: um vazamento derruba a aba (e o vigia a reabre), não a VPS
    pub js_heap_mb: u64,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn num<T: std::str::FromStr>(name: &str, default: T) -> T {
    var(name).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let password = var("PG_PASSWORD");
        if password.is_none() && var("PG_NO_AUTH").as_deref() != Some("1") {
            return Err("Defina PG_PASSWORD (ou PG_NO_AUTH=1 só para teste local)".into());
        }
        if password.as_ref().is_some_and(|p| p.len() < 12) {
            return Err("PG_PASSWORD precisa de pelo menos 12 caracteres: a página controla as suas contas".into());
        }
        let (width, height) = var("PG_VIEWPORT")
            .and_then(|v| {
                let (w, h) = v.split_once(['x', 'X'])?;
                Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
            })
            .unwrap_or((1280, 720));
        let accounts = num("PG_ACCOUNTS", 4usize).clamp(1, 4);
        let names = var("PG_NAMES").map(|v| v.split(',').map(|n| n.trim().to_string()).collect()).unwrap_or_default();
        Ok(Config {
            bind: var("PG_BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
            user: var("PG_USER").unwrap_or_else(|| "pokegrid".into()),
            password,
            data_dir: var("PG_DATA").unwrap_or_else(|| "/data".into()).into(),
            web_dir: var("PG_WEB").unwrap_or_else(|| "/app/web".into()).into(),
            chrome: var("PG_CHROME"),
            game_url: var("PG_GAME_URL").unwrap_or_else(|| "https://poke.idleworld.online".into()),
            accounts,
            names,
            width: width.clamp(640, 1920),
            height: height.clamp(360, 1080),
            fps_grid: num("PG_FPS", 2.0f64).clamp(0.2, 10.0),
            fps_focus: num("PG_FPS_FOCUS", 5.0f64).clamp(0.2, 15.0),
            jpeg_quality: num("PG_JPEG_QUALITY", 50u32).clamp(20, 90),
            mem_account_mb: num("PG_MEM_ACCOUNT_MB", 900u64).max(300),
            mem_total_mb: num("PG_MEM_TOTAL_MB", 4300u64).max(600),
            js_heap_mb: num("PG_JS_HEAP_MB", 768u64).max(256),
        })
    }

    /// Nome da conta `slot` (1..=4); sem nome configurado, "Conta N"
    pub fn name(&self, slot: u8) -> String {
        self.names
            .get(slot as usize - 1)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("Conta {}", slot))
    }
}
