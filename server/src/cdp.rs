//! Cliente mínimo do protocolo DevTools (CDP) sobre WebSocket, com sessões "flatten": um socket por Chromium,
//! e cada aba é endereçada pelo `sessionId` da mensagem.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

pub struct Event {
    pub method: String,
    pub params: Value,
    pub session: Option<String>,
}

pub struct Cdp {
    tx: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next: AtomicU64,
}

impl Cdp {
    /// Conecta no endpoint do navegador. Os eventos chegam pelo receiver; ele fecha quando o socket cai
    pub async fn connect(url: &str) -> Result<(Arc<Cdp>, mpsc::UnboundedReceiver<Event>), String> {
        let (ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| format!("CDP {}: {}", url, e))?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut out) = mpsc::unbounded_channel::<Message>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();

        tokio::spawn(async move {
            while let Some(msg) = out.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(mut v) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(id) = v.get("id").and_then(Value::as_u64) {
                    let Some(done) = reader_pending.lock().ok().and_then(|mut p| p.remove(&id)) else { continue };
                    let result = match v.get("error") {
                        Some(err) => Err(err.get("message").and_then(Value::as_str).unwrap_or("erro do CDP").to_string()),
                        None => Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null)),
                    };
                    let _ = done.send(result);
                } else if let Some(method) = v.get("method").and_then(Value::as_str) {
                    let event = Event {
                        method: method.to_string(),
                        session: v.get("sessionId").and_then(Value::as_str).map(str::to_string),
                        params: v.get_mut("params").map(Value::take).unwrap_or(Value::Null),
                    };
                    if ev_tx.send(event).is_err() {
                        break;
                    }
                }
            }
            // Socket caiu: quem espera resposta recebe erro (em vez de esperar o timeout)
            if let Ok(mut p) = reader_pending.lock() {
                p.clear();
            }
        });

        Ok((Arc::new(Cdp { tx, pending, next: AtomicU64::new(1) }), ev_rx))
    }

    fn message(&self, id: u64, session: Option<&str>, method: &str, params: Value) -> Message {
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = Value::String(s.to_string());
        }
        Message::Text(msg.to_string().into())
    }

    /// Chama um método e espera a resposta
    pub async fn call(&self, session: Option<&str>, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (done, wait) = oneshot::channel();
        self.pending.lock().map_err(|e| e.to_string())?.insert(id, done);
        if self.tx.send(self.message(id, session, method, params)).is_err() {
            return Err("CDP desconectado".into());
        }
        match tokio::time::timeout(CALL_TIMEOUT, wait).await {
            Ok(Ok(result)) => result.map_err(|e| format!("{}: {}", method, e)),
            Ok(Err(_)) => Err(format!("{}: CDP desconectado", method)),
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&id);
                }
                Err(format!("{}: sem resposta em {}s", method, CALL_TIMEOUT.as_secs()))
            }
        }
    }

    /// Manda sem esperar resposta (entrada de mouse/teclado, ack de quadro)
    pub fn send(&self, session: Option<&str>, method: &str, params: Value) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(self.message(id, session, method, params));
    }
}
