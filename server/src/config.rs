//! Configuração por variáveis de ambiente ou pelo arquivo `pokegrid.conf` ao lado do executável (no PC), que
//! vale para o que não estiver no ambiente. Ver `server/README.md`
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

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
    /// Túnel até a VPS (contas rodando no PC): URL do servidor chisel, usuário:senha e a porta aberta lá
    pub tunnel_url: Option<String>,
    pub tunnel_auth: Option<String>,
    pub tunnel_remote: String,
    pub tunnel_bin: Option<String>,
}

/// Pasta do executável (no PC, onde ficam o `pokegrid.conf`, a página e os perfis)
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_else(|| ".".into())
}

/// `CHAVE=valor` do `pokegrid.conf` (ou de `PG_CONF`); linhas vazias e com `#` são ignoradas
fn conf_file() -> &'static HashMap<String, String> {
    static FILE: OnceLock<HashMap<String, String>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("PG_CONF").map(PathBuf::from).unwrap_or_else(|| exe_dir().join("pokegrid.conf"));
        let text = std::fs::read_to_string(path).unwrap_or_default();
        text.lines()
            .map(|l| l.trim_start_matches('\u{feff}').trim())
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
            .collect()
    })
}

fn var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| conf_file().get(name).cloned())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
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
        // No PC (Windows) tudo fica na pasta do executável e a porta só abre para o próprio PC (o acesso de fora
        // vem pelo túnel). No Windows a RAM medida é a memória privada, um pouco maior que o PSS do Linux
        let windows = cfg!(windows);
        let tunnel_url = var("PG_TUNNEL_URL");
        let tunnel_auth = var("PG_TUNNEL_AUTH");
        if tunnel_url.is_some() && tunnel_auth.as_ref().is_none_or(|a| !a.contains(':')) {
            return Err("PG_TUNNEL_AUTH precisa ser usuario:senha (o AUTH do túnel no Dokploy)".into());
        }
        Ok(Config {
            bind: var("PG_BIND").unwrap_or_else(|| if windows { "127.0.0.1:8080" } else { "0.0.0.0:8080" }.into()),
            user: var("PG_USER").unwrap_or_else(|| "pokegrid".into()),
            password,
            data_dir: var("PG_DATA").map(PathBuf::from).unwrap_or_else(|| {
                if windows {
                    exe_dir().join("dados")
                } else {
                    "/data".into()
                }
            }),
            web_dir: var("PG_WEB").map(PathBuf::from).unwrap_or_else(|| {
                if windows {
                    exe_dir().join("web")
                } else {
                    "/app/web".into()
                }
            }),
            chrome: var("PG_CHROME"),
            game_url: var("PG_GAME_URL").unwrap_or_else(|| "https://poke.idleworld.online".into()),
            accounts,
            names,
            width: width.clamp(640, 1920),
            height: height.clamp(360, 1080),
            fps_grid: num("PG_FPS", 2.0f64).clamp(0.2, 10.0),
            fps_focus: num("PG_FPS_FOCUS", 5.0f64).clamp(0.2, 15.0),
            jpeg_quality: num("PG_JPEG_QUALITY", 50u32).clamp(20, 90),
            mem_account_mb: num("PG_MEM_ACCOUNT_MB", if windows { 1200u64 } else { 900 }).max(300),
            mem_total_mb: num("PG_MEM_TOTAL_MB", if windows { 4800u64 } else { 4300 }).max(600),
            js_heap_mb: num("PG_JS_HEAP_MB", 768u64).max(256),
            tunnel_url,
            tunnel_auth,
            tunnel_remote: var("PG_TUNNEL_REMOTE").unwrap_or_else(|| "0.0.0.0:8080".into()),
            tunnel_bin: var("PG_TUNNEL_BIN"),
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
