//! Página web: login (Basic auth), API do painel, vídeo das contas e entrada de mouse/teclado por WebSocket.
use crate::account::{AccountHandle, Cmd, Demand, Status, Video};
use crate::config::Config;
use crate::farm::{export_csv, local_datetime};
use crate::stats::{now_ms, Stats};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;
use tower_http::services::{ServeDir, ServeFile};

/// Senhas erradas toleradas por minuto antes de bloquear o login por um minuto
const MAX_FAILS_PER_MIN: u32 = 10;
const COOKIE: &str = "pg_session";
const PING_EVERY: Duration = Duration::from_secs(25);

pub struct App {
    pub cfg: Arc<Config>,
    pub stats: Arc<Stats>,
    pub video: Arc<Video>,
    pub accounts: Vec<AccountHandle>,
    hub: Mutex<Hub>,
    fails: Mutex<(Instant, u32)>,
    /// Valor do cookie de sessão dado a quem passou pelo login (novo a cada início do servidor)
    session: String,
}

/// Quem está olhando o quê: decide se cada conta transmite (e em que qualidade)
#[derive(Default)]
struct Hub {
    next: u64,
    viewers: HashMap<u64, View>,
    sent: [Demand; 4],
}

#[derive(Default, Clone, Copy)]
struct View {
    slots: [bool; 4],
    focus: u8,
}

impl App {
    pub fn new(cfg: Arc<Config>, stats: Arc<Stats>, video: Arc<Video>, accounts: Vec<AccountHandle>) -> Arc<App> {
        Arc::new(App {
            cfg,
            stats,
            video,
            accounts,
            hub: Mutex::default(),
            fails: Mutex::new((Instant::now(), 0)),
            session: random_token(),
        })
    }

    fn account(&self, slot: u8) -> Option<&AccountHandle> {
        self.accounts.iter().find(|a| a.slot == slot)
    }

    fn set_view(&self, id: u64, view: Option<View>) {
        let Ok(mut hub) = self.hub.lock() else { return };
        match view {
            Some(v) => hub.viewers.insert(id, v),
            None => hub.viewers.remove(&id),
        };
        for i in 0..4 {
            let slot = i as u8 + 1;
            let demand = if hub.viewers.values().any(|v| v.focus == slot) {
                Demand::Focus
            } else if hub.viewers.values().any(|v| v.slots[i]) {
                Demand::Grid
            } else {
                Demand::Off
            };
            if hub.sent[i] != demand {
                hub.sent[i] = demand;
                if let Some(acc) = self.account(slot) {
                    let _ = acc.cmd.send(Cmd::Demand(demand));
                }
            }
        }
    }

    /// Senha errada demais no último minuto: o login fica bloqueado até o minuto virar
    fn locked(&self, failed: bool) -> bool {
        let Ok(mut f) = self.fails.lock() else { return true };
        if f.0.elapsed() > Duration::from_secs(60) {
            *f = (Instant::now(), 0);
        }
        if failed {
            f.1 += 1;
        }
        f.1 >= MAX_FAILS_PER_MIN
    }
}

pub fn router(app: Arc<App>) -> Router {
    let web = &app.cfg.web_dir;
    Router::new()
        .route_service("/", ServeFile::new(web.join("remote.html")))
        .route("/api/info", get(info))
        .route("/api/snapshot", get(snapshot))
        .route("/api/reset", post(reset))
        .route("/api/export", get(export))
        .route("/api/reload/{slot}", post(reload))
        .route("/ws", get(ws))
        .fallback_service(ServeDir::new(web))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    origin.split_once("://").map(|(_, h)| h.trim_end_matches('/')) == Some(host)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_token() -> String {
    let mut buf = [0u8; 32];
    if let Err(err) = getrandom::fill(&mut buf) {
        eprintln!("[web] sem gerador aleatório do sistema: {}", err);
        std::process::exit(2);
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

/// Login em tudo (página, API e WebSocket), bloqueio contra força bruta e pedidos só da própria origem.
/// Quem passa pelo Basic auth ganha um cookie de sessão, que vale também no WebSocket (o Safari do iOS não
/// repete o Basic auth lá)
async fn guard(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    let mut new_session = false;
    let has_session =
        session_cookie(req.headers()).is_some_and(|c| constant_eq(c.as_bytes(), app.session.as_bytes()));
    if let (Some(password), false) = (&app.cfg.password, has_session) {
        let given = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Basic "))
            .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v.trim()).ok());
        let expected = format!("{}:{}", app.cfg.user, password);
        let ok = given.as_deref().is_some_and(|g| constant_eq(g, expected.as_bytes()));
        if app.locked(given.is_some() && !ok) {
            return (StatusCode::TOO_MANY_REQUESTS, "Muitas tentativas. Espere um minuto.").into_response();
        }
        if !ok {
            if given.is_some() {
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            let mut res = (StatusCode::UNAUTHORIZED, "Login necessário").into_response();
            res.headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"PokeGrid\", charset=\"UTF-8\""));
            return res;
        }
        new_session = true;
    }
    // Outro site não pode usar o login guardado no navegador para mandar cliques ou zerar o painel
    let upgrade = req.headers().contains_key(header::UPGRADE);
    if (req.method() != Method::GET || upgrade) && !same_origin(req.headers()) {
        return (StatusCode::FORBIDDEN, "Origem não permitida").into_response();
    }
    let api = req.uri().path().starts_with("/api/");
    let https = req.headers().get("x-forwarded-proto").is_some_and(|v| v.as_bytes() == b"https");
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    if new_session {
        let cookie = format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{}",
            COOKIE,
            app.session,
            if https { "; Secure" } else { "" }
        );
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            h.append(header::SET_COOKIE, v);
        }
    }
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

async fn info(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let accounts: Vec<Status> = app.accounts.iter().map(|a| a.status.borrow().clone()).collect();
    let names: Vec<String> = (1..=4).map(|s| app.cfg.name(s)).collect();
    Json(json!({
        "names": names,
        "accounts": accounts,
        "width": app.cfg.width,
        "height": app.cfg.height,
        "memAccountMb": app.cfg.mem_account_mb,
        "memTotalMb": app.cfg.mem_total_mb,
    }))
}

#[derive(Deserialize)]
struct TzQuery {
    #[serde(default)]
    tz: i32,
}

async fn snapshot(State(app): State<Arc<App>>, Query(q): Query<TzQuery>) -> Response {
    let snap = match app.stats.farm.lock() {
        Ok(farm) => farm.snapshot(now_ms(), q.tz),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let mut snap = snap;
    for a in &mut snap.accounts {
        a.hunt_name = app.stats.hunt_name(&a.hunt_slug);
    }
    Json(snap).into_response()
}

async fn reset(State(app): State<Arc<App>>) -> StatusCode {
    match app.stats.farm.lock() {
        Ok(mut farm) => {
            farm.reset_sessions(now_ms());
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct ExportQuery {
    kind: String,
    #[serde(default)]
    tz: i32,
}

async fn export(State(app): State<Arc<App>>, Query(q): Query<ExportQuery>) -> Response {
    let now = now_ms();
    let result = match app.stats.farm.lock() {
        Ok(farm) => export_csv(&farm.history, &q.kind, now, q.tz, |a| app.cfg.name(a), |s| app.stats.hunt_name(s)),
        Err(e) => Err(e.to_string()),
    };
    match result {
        Ok((label, csv)) => {
            let stamp = local_datetime(now, q.tz).replace([' ', ':'], "_");
            let disposition = format!("attachment; filename=\"pokegrid-{}-{}.csv\"", label, stamp);
            Response::builder()
                .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
                .header(header::CONTENT_DISPOSITION, disposition)
                .body(Body::from(csv))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(err) => (StatusCode::BAD_REQUEST, err).into_response(),
    }
}

async fn reload(State(app): State<Arc<App>>, Path(slot): Path<u8>) -> StatusCode {
    match app.account(slot) {
        Some(acc) if acc.cmd.send(Cmd::Reload).is_ok() => StatusCode::NO_CONTENT,
        _ => StatusCode::NOT_FOUND,
    }
}

async fn ws(State(app): State<Arc<App>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.max_message_size(64 * 1024).on_upgrade(move |socket| viewer(app, socket))
}

/// Mensagens da página: o que ela está mostrando e a entrada de quem está nela
#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum ClientMsg {
    View {
        slots: Vec<u8>,
        #[serde(default)]
        focus: u8,
    },
    Mouse {
        s: u8,
        k: String,
        x: f64,
        y: f64,
        #[serde(default)]
        b: u8,
        #[serde(default)]
        c: u32,
    },
    Wheel {
        s: u8,
        x: f64,
        y: f64,
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
    },
    Key {
        s: u8,
        k: String,
        key: String,
        #[serde(default)]
        code: String,
        #[serde(default)]
        m: u32,
    },
    Text {
        s: u8,
        text: String,
    },
}

async fn viewer(app: Arc<App>, socket: WebSocket) {
    let id = {
        let Ok(mut hub) = app.hub.lock() else { return };
        hub.next += 1;
        hub.next
    };
    let (mut sink, mut stream) = socket.split();
    let mut frames = app.video.tx.subscribe();
    let mut view = View::default();
    let mut ping = tokio::time::interval(PING_EVERY);

    loop {
        tokio::select! {
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                };
                let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else { continue };
                let (slot, cmd) = match msg {
                    ClientMsg::View { slots, focus } => {
                        let before = view;
                        view = View { slots: [false; 4], focus: if (1..=4).contains(&focus) { focus } else { 0 } };
                        for s in slots.into_iter().filter(|s| (1..=4).contains(s)) {
                            view.slots[s as usize - 1] = true;
                        }
                        app.set_view(id, Some(view));
                        // Conta que acabou de aparecer: manda já o último quadro dela
                        for s in 1..=4u8 {
                            let i = s as usize - 1;
                            if view.slots[i] && !before.slots[i] {
                                if let Some(frame) = app.video.latest(s) {
                                    if sink.send(Message::Binary(frame)).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    ClientMsg::Mouse { s, k, x, y, b, c } => (s, Cmd::Mouse { kind: k, x, y, button: b, count: c }),
                    ClientMsg::Wheel { s, x, y, dx, dy } => (s, Cmd::Wheel { x, y, dx, dy }),
                    ClientMsg::Key { s, k, key, code, m } => (s, Cmd::Key { kind: k, key, code, mods: m }),
                    ClientMsg::Text { s, text } => (s, Cmd::Text(text)),
                };
                if let Some(acc) = app.account(slot) {
                    let _ = acc.cmd.send(cmd);
                }
            }
            frame = frames.recv() => {
                match frame {
                    Ok(frame) => {
                        let slot = frame.first().copied().unwrap_or(0) as usize;
                        if (1..=4).contains(&slot) && view.slots[slot - 1] && sink.send(Message::Binary(frame)).await.is_err() {
                            break;
                        }
                    }
                    // Conexão lenta: perdeu quadros velhos, segue do mais novo
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }
    app.set_view(id, None);
}
