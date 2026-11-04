//! Uma conta: o seu Chromium, a aba do jogo, o vídeo (screencast) para quem estiver olhando, a entrada de
//! mouse/teclado vinda da página e a leitura passiva do WebSocket do jogo para o painel.
//!
//! Nada de script roda na página do jogo: o servidor só usa o protocolo DevTools para ver a tela (como um
//! print), repassar os cliques e o teclado de quem está na página e ler a cópia das mensagens do WebSocket.
use crate::cdp::{Cdp, Event};
use crate::chrome::{self, Chrome};
use crate::config::Config;
use crate::stats::{now_ms, Stats};
use base64::Engine;
use bytes::Bytes;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::sleep_until;

/// De quanto em quanto tempo mede a RAM da conta
const MEM_EVERY: Duration = Duration::from_secs(30);
/// Recarga pedida pelo vigia no meio de uma hunt: espera um abate (fim de luta) até este limite
const RECYCLE_WAIT: Duration = Duration::from_secs(90);
/// Recarregou há menos que isto e a RAM continua alta: o vazamento não é da página, reinicia o Chromium
const ESCALATE_WITHIN: Duration = Duration::from_secs(10 * 60);
/// Largura máxima do vídeo na grade (a tela inteira vai só com a conta ampliada)
const GRID_MAX_WIDTH: u32 = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Demand {
    #[default]
    Off,
    Grid,
    Focus,
}

pub enum Cmd {
    Demand(Demand),
    Mouse { kind: String, x: f64, y: f64, button: u8, count: u32 },
    Wheel { x: f64, y: f64, dx: f64, dy: f64 },
    Key { kind: String, key: String, code: String, mods: u32 },
    Text(String),
    /// Recarregar agora (botão da página)
    Reload,
    /// Pedido do vigia de memória; `urgent` recarrega sem esperar o fim da luta
    Recycle { reason: String, urgent: bool },
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub slot: u8,
    /// starting | running | restarting
    pub state: &'static str,
    pub mem_mb: Option<u64>,
    pub reloads: u32,
    pub restarts: u32,
    /// Última recarga ou reinício feito pelo vigia de memória (motivo e quando)
    pub last_recycle: Option<String>,
    pub last_recycle_at: u64,
    pub url: String,
    /// Uma janela aberta pelo jogo (login, por exemplo) está na frente da aba do jogo
    pub popup: bool,
    pub error: Option<String>,
}

/// Quadros JPEG de todas as contas. Cada mensagem é `[slot][jpeg]`; quem fica para trás perde quadros
/// velhos (broadcast com fila curta) em vez de acumular memória
pub struct Video {
    pub tx: broadcast::Sender<Bytes>,
    latest: Mutex<Vec<Option<Bytes>>>,
}

impl Video {
    pub fn new() -> Arc<Video> {
        Arc::new(Video { tx: broadcast::channel(8).0, latest: Mutex::new(vec![None; 4]) })
    }

    fn publish(&self, slot: u8, jpeg: &[u8]) {
        let mut frame = Vec::with_capacity(jpeg.len() + 1);
        frame.push(slot);
        frame.extend_from_slice(jpeg);
        let frame = Bytes::from(frame);
        if let Ok(mut latest) = self.latest.lock() {
            latest[slot as usize - 1] = Some(frame.clone());
        }
        let _ = self.tx.send(frame);
    }

    /// Último quadro da conta, para quem acabou de abrir a página não ver tela preta
    pub fn latest(&self, slot: u8) -> Option<Bytes> {
        self.latest.lock().ok()?.get(slot as usize - 1)?.clone()
    }
}

pub struct AccountHandle {
    pub slot: u8,
    pub cmd: mpsc::UnboundedSender<Cmd>,
    pub status: watch::Receiver<Status>,
}

pub fn spawn(slot: u8, cfg: Arc<Config>, stats: Arc<Stats>, video: Arc<Video>) -> AccountHandle {
    let (cmd, cmd_rx) = mpsc::unbounded_channel();
    let (status_tx, status) = watch::channel(Status { slot, state: "starting", ..Default::default() });
    tokio::spawn(run(slot, cfg, stats, video, cmd_rx, status_tx));
    AccountHandle { slot, cmd, status }
}

async fn run(
    slot: u8,
    cfg: Arc<Config>,
    stats: Arc<Stats>,
    video: Arc<Video>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    status: watch::Sender<Status>,
) {
    let mut demand = Demand::Off;
    let mut failures = 0u32;
    loop {
        status.send_modify(|s| s.state = "starting");
        let started = Instant::now();
        let err = match Page::open(slot, &cfg, &stats, &video, &status, demand).await {
            Ok((mut page, mut chrome)) => {
                status.send_modify(|s| {
                    s.state = "running";
                    s.error = None;
                });
                let err = page.serve(&mut chrome, &mut cmd_rx, &mut demand).await;
                let _ = chrome.child.kill().await;
                match err {
                    Some(err) => err,
                    // O canal de comandos fechou: o servidor está desligando
                    None => return,
                }
            }
            Err(err) => err,
        };
        eprintln!("[conta{}] {}", slot, err);
        failures = if started.elapsed() > Duration::from_secs(300) { 1 } else { failures + 1 };
        status.send_modify(|s| {
            s.state = "restarting";
            s.restarts += 1;
            s.mem_mb = None;
            s.error = Some(err);
        });
        tokio::time::sleep(Duration::from_secs((1u64 << failures.min(6)).min(60))).await;
    }
}

/// (sessão do CDP, `sessionId` do screencast) para devolver o ack de um quadro mais tarde
struct Ack(String, Value);

struct Page<'a> {
    slot: u8,
    cfg: &'a Config,
    stats: &'a Stats,
    video: &'a Video,
    status: &'a watch::Sender<Status>,
    cdp: Arc<Cdp>,
    events: mpsc::UnboundedReceiver<Event>,
    ua: Value,
    main: String,
    main_target: String,
    /// (alvo, sessão) de uma janela aberta pelo jogo
    popup: Option<(String, String)>,
    /// Sessão com screencast ligado
    casting: Option<String>,
    demand: Demand,
    last_frame: Instant,
    pressed: u32,
    /// Recarga do vigia esperando o fim da luta
    recycle: Option<String>,
    reload_at: Option<tokio::time::Instant>,
    last_recycle: Option<Instant>,
}

impl<'a> Page<'a> {
    async fn open(
        slot: u8,
        cfg: &'a Config,
        stats: &'a Stats,
        video: &'a Video,
        status: &'a watch::Sender<Status>,
        demand: Demand,
    ) -> Result<(Page<'a>, Chrome), String> {
        let chrome = chrome::launch(cfg, slot).await?;
        let (cdp, events) = Cdp::connect(&chrome.ws_url).await?;

        // Sem "HeadlessChrome" no user agent: para o jogo é um Chrome comum no Linux
        let version = cdp.call(None, "Browser.getVersion", json!({})).await?;
        let product = version["product"].as_str().unwrap_or("Chrome/0").to_string();
        let full = product.split('/').nth(1).unwrap_or("0").to_string();
        let major = full.split('.').next().unwrap_or("0").to_string();
        let ua = json!({
            "userAgent": version["userAgent"].as_str().unwrap_or_default().replace("HeadlessChrome", "Chrome"),
            "acceptLanguage": "pt-BR,pt;q=0.9,en;q=0.8",
            "platform": "Linux x86_64",
            "userAgentMetadata": {
                "brands": [
                    { "brand": "Chromium", "version": major },
                    { "brand": "Not.A/Brand", "version": "99" }
                ],
                "fullVersionList": [
                    { "brand": "Chromium", "version": full },
                    { "brand": "Not.A/Brand", "version": "99.0.0.0" }
                ],
                "fullVersion": full,
                "platform": "Linux",
                "platformVersion": "",
                "architecture": "x86",
                "model": "",
                "mobile": false
            }
        });

        cdp.call(None, "Target.setDiscoverTargets", json!({ "discover": true })).await?;
        let targets = cdp.call(None, "Target.getTargets", json!({})).await?;
        let existing = targets["targetInfos"]
            .as_array()
            .and_then(|t| t.iter().find(|t| t["type"] == "page"))
            .and_then(|t| t["targetId"].as_str())
            .map(str::to_string);
        let main_target = match existing {
            Some(id) => id,
            None => cdp.call(None, "Target.createTarget", json!({ "url": "about:blank" })).await?["targetId"]
                .as_str()
                .ok_or("Target.createTarget sem targetId")?
                .to_string(),
        };

        let mut page = Page {
            slot,
            cfg,
            stats,
            video,
            status,
            cdp,
            events,
            ua,
            main: String::new(),
            main_target: main_target.clone(),
            popup: None,
            casting: None,
            demand,
            last_frame: Instant::now(),
            pressed: 0,
            recycle: None,
            reload_at: None,
            last_recycle: None,
        };
        page.main = page.attach(&main_target).await?;
        // Network só com eventos (buffers 0): o corpo das respostas não fica guardado em RAM
        let main = Some(page.main.as_str());
        let buffers = json!({ "maxTotalBufferSize": 0, "maxResourceBufferSize": 0 });
        if page.cdp.call(main, "Network.enable", buffers).await.is_err() {
            page.cdp.call(main, "Network.enable", json!({})).await?;
        }
        page.cdp.call(main, "Page.navigate", json!({ "url": cfg.game_url })).await?;
        page.restart_cast().await;
        Ok((page, chrome))
    }

    /// Conecta numa aba e deixa ela com o tamanho, o foco e o user agent certos
    async fn attach(&self, target: &str) -> Result<String, String> {
        let res = self.cdp.call(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true })).await?;
        let session = res["sessionId"].as_str().ok_or("Target.attachToTarget sem sessionId")?.to_string();
        let s = Some(session.as_str());
        self.cdp.call(s, "Page.enable", json!({})).await?;
        // Ajustes que melhoram mas não são essenciais: se a versão do Chromium recusar, a conta abre assim mesmo
        let metrics = json!({ "width": self.cfg.width, "height": self.cfg.height, "deviceScaleFactor": 1, "mobile": false });
        let optional = [
            ("Emulation.setUserAgentOverride", self.ua.clone()),
            ("Emulation.setDeviceMetricsOverride", metrics),
            // A página se comporta como a janela em primeiro plano (jogos costumam pausar sem foco)
            ("Emulation.setFocusEmulationEnabled", json!({ "enabled": true })),
        ];
        for (method, params) in optional {
            if let Err(err) = self.cdp.call(s, method, params).await {
                eprintln!("[conta{}] {}", self.slot, err);
            }
        }
        Ok(session)
    }

    fn active(&self) -> &str {
        self.popup.as_ref().map(|(_, s)| s.as_str()).unwrap_or(&self.main)
    }

    /// Liga, desliga ou troca o screencast conforme quem está olhando e qual aba está na frente
    async fn restart_cast(&mut self) {
        if let Some(s) = self.casting.take() {
            self.cdp.send(Some(&s), "Page.stopScreencast", json!({}));
        }
        if self.demand == Demand::Off {
            return;
        }
        let (w, h) = (self.cfg.width, self.cfg.height);
        let (max_w, quality) = match self.demand {
            Demand::Focus => (w, (self.cfg.jpeg_quality + 10).min(90)),
            _ => (w.min(GRID_MAX_WIDTH), self.cfg.jpeg_quality),
        };
        let max_h = (max_w as u64 * h as u64 / w as u64) as u32;
        let session = self.active().to_string();
        self.cdp.send(
            Some(&session),
            "Page.startScreencast",
            json!({ "format": "jpeg", "quality": quality, "maxWidth": max_w, "maxHeight": max_h, "everyNthFrame": 1 }),
        );
        self.casting = Some(session.clone());

        // O screencast só manda quadro quando a tela muda: um print agora para quem acabou de chegar
        let shot = json!({
            "format": "jpeg",
            "quality": quality,
            "optimizeForSpeed": true,
            "clip": { "x": 0, "y": 0, "width": w, "height": h, "scale": max_w as f64 / w as f64 }
        });
        if let Some(jpeg) = self.cdp.call(Some(&session), "Page.captureScreenshot", shot).await.ok().and_then(|v| decode(&v["data"])) {
            self.video.publish(self.slot, &jpeg);
        }
    }

    fn reload(&mut self, why: Option<String>) {
        self.cdp.send(Some(&self.main), "Page.reload", json!({}));
        self.recycle = None;
        self.reload_at = None;
        if why.is_some() {
            self.last_recycle = Some(Instant::now());
        }
        self.status.send_modify(|s| {
            s.reloads += 1;
            if let Some(why) = why {
                s.last_recycle = Some(format!("recarregada: {}", why));
                s.last_recycle_at = now_ms();
            }
        });
    }

    /// Roda até o Chromium cair (devolve o motivo) ou o servidor desligar (`None`)
    async fn serve(
        &mut self,
        chrome: &mut Chrome,
        cmd_rx: &mut mpsc::UnboundedReceiver<Cmd>,
        demand: &mut Demand,
    ) -> Option<String> {
        let mut mem_tick = tokio::time::interval(MEM_EVERY);
        let mut acks: VecDeque<(tokio::time::Instant, Ack)> = VecDeque::new();
        loop {
            let far = tokio::time::Instant::now() + Duration::from_secs(3600);
            let next_ack = acks.front().map(|(t, _)| *t);
            tokio::select! {
                ev = self.events.recv() => {
                    let Some(ev) = ev else { return Some("a conexão com o Chromium caiu".into()) };
                    if let Err(err) = self.on_event(ev, &mut acks).await {
                        return Some(err);
                    }
                }
                cmd = cmd_rx.recv() => {
                    let cmd = cmd?;
                    if let Cmd::Demand(d) = &cmd {
                        *demand = *d;
                    }
                    if let Err(err) = self.on_cmd(cmd).await {
                        return Some(err);
                    }
                }
                _ = sleep_until(next_ack.unwrap_or(far)), if next_ack.is_some() => {
                    if let Some((_, Ack(session, id))) = acks.pop_front() {
                        self.cdp.send(Some(&session), "Page.screencastFrameAck", json!({ "sessionId": id }));
                    }
                }
                _ = sleep_until(self.reload_at.unwrap_or(far)), if self.reload_at.is_some() => {
                    let why = self.recycle.take();
                    self.reload(why);
                }
                _ = mem_tick.tick() => {
                    let pid = chrome.pid;
                    let mb = tokio::task::spawn_blocking(move || crate::mem::tree_mb(pid)).await.ok().flatten();
                    self.status.send_modify(|s| s.mem_mb = mb);
                }
                exit = chrome.child.wait() => {
                    return Some(format!("o Chromium saiu ({})", exit.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string())));
                }
            }
        }
    }

    async fn on_event(&mut self, ev: Event, acks: &mut VecDeque<(tokio::time::Instant, Ack)>) -> Result<(), String> {
        let from_main = ev.session.as_deref() == Some(self.main.as_str());
        let p = &ev.params;
        match ev.method.as_str() {
            "Page.screencastFrame" => {
                let Some(session) = ev.session.clone() else { return Ok(()) };
                if self.casting.as_deref() != Some(session.as_str()) {
                    return Ok(());
                }
                if let Some(jpeg) = decode(&p["data"]) {
                    self.video.publish(self.slot, &jpeg);
                }
                // O próximo quadro só vem depois do ack: segurar o ack é o que limita os quadros por segundo
                let fps = if self.demand == Demand::Focus { self.cfg.fps_focus } else { self.cfg.fps_grid };
                let wait = Duration::from_secs_f64(1.0 / fps).saturating_sub(self.last_frame.elapsed());
                self.last_frame = Instant::now();
                acks.push_back((tokio::time::Instant::now() + wait, Ack(session, p["sessionId"].clone())));
            }
            "Network.webSocketCreated" if from_main => {
                if let (Some(id), Some(url), Ok(mut farm)) =
                    (p["requestId"].as_str(), p["url"].as_str(), self.stats.farm.lock())
                {
                    farm.socket_created(self.slot as usize, id, url);
                }
            }
            "Network.webSocketFrameReceived" if from_main => {
                // Só frames de texto (opcode 1): o jogo manda JSON
                let (Some(id), Some(data)) = (p["requestId"].as_str(), p["response"]["payloadData"].as_str()) else {
                    return Ok(());
                };
                if p["response"]["opcode"].as_f64() != Some(1.0) {
                    return Ok(());
                }
                if let Ok(mut farm) = self.stats.farm.lock() {
                    farm.frame(self.slot as usize, id, data, now_ms());
                }
                // Recarga pendente: um abate acabou de encerrar a luta, é a hora de recarregar
                if self.recycle.is_some() && data.contains("\"field-kill\"") {
                    self.reload_at = Some(tokio::time::Instant::now() + Duration::from_millis(1500));
                }
            }
            "Network.webSocketClosed" if from_main => {
                if let (Some(id), Ok(mut farm)) = (p["requestId"].as_str(), self.stats.farm.lock()) {
                    farm.socket_closed(self.slot as usize, id, now_ms());
                }
            }
            "Page.frameNavigated" if from_main => {
                if p["frame"]["parentId"].is_null() {
                    let url = p["frame"]["url"].as_str().unwrap_or_default();
                    let url = url.split(['?', '#']).next().unwrap_or_default().to_string();
                    self.status.send_modify(|s| s.url = url);
                }
            }
            // alert/confirm travariam a aba sem ninguém para clicar (inclusive o "sair da página?" de uma recarga)
            "Page.javascriptDialogOpening" => {
                self.cdp.send(ev.session.as_deref(), "Page.handleJavaScriptDialog", json!({ "accept": true }));
            }
            "Inspector.targetCrashed" if from_main => {
                eprintln!("[conta{}] a aba do jogo travou; recarregando", self.slot);
                self.reload(Some("a aba travou".into()));
            }
            "Target.targetCreated" => {
                let info = &p["targetInfo"];
                let id = info["targetId"].as_str().unwrap_or_default().to_string();
                if info["type"] == "page" && id != self.main_target && self.popup.is_none() {
                    // Janela aberta pelo jogo (login por outro site, por exemplo): vai para a frente da tela
                    if let Ok(session) = self.attach(&id).await {
                        self.popup = Some((id, session));
                        self.status.send_modify(|s| s.popup = true);
                        self.restart_cast().await;
                    }
                }
            }
            "Target.targetDestroyed" => {
                let id = p["targetId"].as_str().unwrap_or_default();
                if id == self.main_target {
                    return Err("a aba do jogo fechou".into());
                }
                if self.popup.as_ref().is_some_and(|(t, _)| t == id) {
                    self.popup = None;
                    self.status.send_modify(|s| s.popup = false);
                    self.restart_cast().await;
                }
            }
            "Target.detachedFromTarget" if p["sessionId"].as_str() == Some(self.main.as_str()) => {
                return Err("o DevTools perdeu a aba do jogo".into());
            }
            _ => {}
        }
        Ok(())
    }

    async fn on_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let (w, h) = (self.cfg.width as f64, self.cfg.height as f64);
        match cmd {
            Cmd::Demand(d) => {
                if d != self.demand {
                    self.demand = d;
                    self.restart_cast().await;
                }
            }
            Cmd::Mouse { kind, x, y, button, count } => {
                let (name, bit) = match button {
                    1 => ("middle", 4),
                    2 => ("right", 2),
                    _ => ("left", 1),
                };
                let (ty, button) = match kind.as_str() {
                    "down" => {
                        self.pressed |= bit;
                        ("mousePressed", name)
                    }
                    "up" => {
                        self.pressed &= !bit;
                        ("mouseReleased", name)
                    }
                    _ => ("mouseMoved", if self.pressed == 0 { "none" } else { name }),
                };
                let params = json!({
                    "type": ty,
                    "x": (x.clamp(0.0, 1.0) * w).round(),
                    "y": (y.clamp(0.0, 1.0) * h).round(),
                    "button": button,
                    "buttons": self.pressed,
                    "clickCount": if ty == "mouseMoved" { 0 } else { count.clamp(1, 3) },
                });
                self.cdp.send(Some(self.active()), "Input.dispatchMouseEvent", params);
            }
            Cmd::Wheel { x, y, dx, dy } => {
                let params = json!({
                    "type": "mouseWheel",
                    "x": (x.clamp(0.0, 1.0) * w).round(),
                    "y": (y.clamp(0.0, 1.0) * h).round(),
                    "deltaX": dx.clamp(-2000.0, 2000.0),
                    "deltaY": dy.clamp(-2000.0, 2000.0),
                });
                self.cdp.send(Some(self.active()), "Input.dispatchMouseEvent", params);
            }
            Cmd::Key { kind, key, code, mods } => {
                // Ctrl/Meta + letra é atalho (copiar, selecionar tudo): vai sem texto
                let printable = key.chars().count() == 1 && mods & (2 | 4) == 0;
                let ty = match (kind.as_str(), printable) {
                    ("down", true) => "keyDown",
                    ("down", false) => "rawKeyDown",
                    _ => "keyUp",
                };
                let mut params = json!({
                    "type": ty,
                    "key": key,
                    "code": code,
                    "modifiers": mods & 15,
                    "windowsVirtualKeyCode": virtual_key(&key, &code),
                });
                if ty == "keyDown" {
                    params["text"] = Value::String(key.clone());
                    params["unmodifiedText"] = Value::String(key);
                }
                self.cdp.send(Some(self.active()), "Input.dispatchKeyEvent", params);
            }
            Cmd::Text(text) => {
                let text: String = text.chars().take(2000).collect();
                self.cdp.send(Some(self.active()), "Input.insertText", json!({ "text": text }));
            }
            Cmd::Reload => self.reload(None),
            Cmd::Recycle { reason, urgent } => {
                // Acabou de recarregar e não adiantou: o vazamento está fora da página, reinicia o navegador
                if self.last_recycle.is_some_and(|t| t.elapsed() < ESCALATE_WITHIN) {
                    self.status.send_modify(|s| {
                        s.last_recycle = Some(format!("reiniciada: {}", reason));
                        s.last_recycle_at = now_ms();
                    });
                    return Err(format!("reiniciando o Chromium: {}", reason));
                }
                if self.recycle.is_some() && !urgent {
                    return Ok(());
                }
                let in_hunt = self.stats.farm.lock().map(|f| f.in_hunt(self.slot as usize)).unwrap_or(false);
                let wait = if urgent || !in_hunt { Duration::ZERO } else { RECYCLE_WAIT };
                self.recycle = Some(reason);
                self.reload_at = Some(tokio::time::Instant::now() + wait);
            }
        }
        Ok(())
    }
}

fn decode(data: &Value) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(data.as_str()?).ok()
}

/// Código de tecla do Windows que o Chromium espera junto do evento (Enter, setas etc. dependem dele)
fn virtual_key(key: &str, code: &str) -> u32 {
    match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" => 13,
        "Shift" => 16,
        "Control" => 17,
        "Alt" => 18,
        "Escape" => 27,
        " " => 32,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Delete" => 46,
        _ => {
            if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1) {
                return c.as_bytes()[0] as u32;
            }
            if let Some(d) = code.strip_prefix("Digit").filter(|d| d.len() == 1) {
                return d.as_bytes()[0] as u32;
            }
            if let Some(f) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=12).contains(n)) {
                return 111 + f;
            }
            key.chars().next().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase() as u32).unwrap_or(0)
        }
    }
}

/// O vigia de memória: a cada 30 s olha a RAM das contas e manda recarregar a maior que passou do teto
/// (dela ou da soma). Uma ação por vez, com intervalo, para a conta recarregada voltar ao normal antes
pub fn spawn_watchdog(cfg: Arc<Config>, accounts: Vec<(mpsc::UnboundedSender<Cmd>, watch::Receiver<Status>)>) {
    const COOLDOWN: Duration = Duration::from_secs(150);
    tokio::spawn(async move {
        let mut last: Option<Instant> = None;
        let mut tick = tokio::time::interval(MEM_EVERY);
        loop {
            tick.tick().await;
            let mems: Vec<(usize, u64)> =
                accounts.iter().enumerate().filter_map(|(i, (_, st))| Some((i, st.borrow().mem_mb?))).collect();
            let total: u64 = mems.iter().map(|(_, mb)| mb).sum();
            let Some(&(biggest, big_mb)) = mems.iter().max_by_key(|(_, mb)| *mb) else { continue };
            // Muito acima do teto somado: não espera o intervalo nem o fim da luta
            let urgent = total > cfg.mem_total_mb + cfg.mem_total_mb * 15 / 100;
            if !urgent && last.is_some_and(|t| t.elapsed() < COOLDOWN) {
                continue;
            }
            let reason = if big_mb > cfg.mem_account_mb {
                format!("{} MB, acima do teto de {} MB por conta", big_mb, cfg.mem_account_mb)
            } else if total > cfg.mem_total_mb && big_mb > 300 {
                format!("contas somando {} MB, acima do teto de {} MB", total, cfg.mem_total_mb)
            } else {
                continue;
            };
            eprintln!("[vigia] conta{}: {}", biggest + 1, reason);
            let _ = accounts[biggest].0.send(Cmd::Recycle { reason, urgent });
            last = Some(Instant::now());
        }
    });
}
