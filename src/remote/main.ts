// PokeGrid Remoto: telas das contas que rodam na VPS (server/), com mouse, toque e teclado repassados, e o
// painel do Modo Eco. O vídeo é JPEG por WebSocket (`[slot][jpeg]`); só transmite o que está na tela.
import { initFarmPanel, type FarmApi, type Snapshot } from "../farm";
import { h } from "../tools/ui";

interface AccountStatus {
  slot: number;
  state: "starting" | "running" | "restarting";
  memMb: number | null;
  reloads: number;
  restarts: number;
  lastRecycle: string | null;
  lastRecycleAt: number;
  url: string;
  popup: boolean;
  error: string | null;
}

interface Info {
  names: string[];
  accounts: AccountStatus[];
  width: number;
  height: number;
  memAccountMb: number;
  memTotalMb: number;
}

const INFO_MS = 5000;
const MOVE_MS = 50;
const SLOT_COLORS = ["#3b82f6", "#10b981", "#f59e0b", "#a855f7"];
/// Toque que andou mais que isto vira rolagem (roda do mouse) em vez de clique
const TAP_SLOP = 10;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const screens = $<HTMLElement>("screens");
const ramTag = $<HTMLSpanElement>("rm-ram");
const linkTag = $<HTMLSpanElement>("rm-link");
const panelBtn = $<HTMLButtonElement>("btn-panel");
const kbdSink = $<HTMLTextAreaElement>("kbd-sink");

let info: Info | null = null;
let focus = 0;
let selected = 1;
let panelOpen = false;
let ws: WebSocket | null = null;
let retry = 0;

// ---------- API HTTP ----------
async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, { credentials: "same-origin", ...init });
  // Login expirou ou trocou a senha: recarregar abre o pedido de senha do navegador
  if (res.status === 401) location.reload();
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`);
  return (res.status === 204 ? undefined : await res.json()) as T;
}

const farmApi: FarmApi = {
  snapshot: (tz) => api<Snapshot>(`/api/snapshot?tz=${tz}`),
  reset: () => api<void>("/api/reset", { method: "POST" }),
  exportCsv: async (kind, tz) => {
    const a = h("a", { href: `/api/export?kind=${kind}&tz=${tz}`, download: "" });
    document.body.append(a);
    a.click();
    a.remove();
    return "Planilha baixada (pasta Downloads do navegador)";
  },
};

const names = () => info?.names ?? [];
const accountName = (slot: number) => names()[slot - 1]?.trim() || `Conta ${slot}`;

// ---------- telas ----------
interface Tile {
  slot: number;
  root: HTMLDivElement;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  name: HTMLSpanElement;
  badge: HTMLSpanElement;
  msg: HTMLDivElement;
  decoding: boolean;
  pending: Blob | null;
  hasFrame: boolean;
}

const tiles: Tile[] = [];

function buildTiles(count: number) {
  screens.replaceChildren();
  tiles.length = 0;
  for (let slot = 1; slot <= count; slot++) {
    const canvas = h("canvas", { class: "rm-canvas" });
    const name = h("span", { class: "rm-name" });
    const badge = h("span", { class: "fp-status muted" }, "—");
    const zoom = h("button", { class: "rm-zoom", title: "Ampliar / voltar à grade" }, "⤢");
    const msg = h("div", { class: "rm-msg" }, "Conectando…");
    const root = h(
      "div",
      { class: "rm-tile", "data-slot": slot },
      canvas,
      msg,
      h("div", { class: "rm-bar" }, h("span", { class: "fp-dot", style: `background:${SLOT_COLORS[slot - 1]}` }), name, badge, zoom),
    );
    const tile: Tile = {
      slot,
      root,
      canvas,
      ctx: canvas.getContext("2d")!,
      name,
      badge,
      msg,
      decoding: false,
      pending: null,
      hasFrame: false,
    };
    zoom.addEventListener("click", () => setFocus(focus === slot ? 0 : slot));
    bindInput(tile);
    tiles.push(tile);
    screens.append(root);
  }
  renderTiles();
}

/// Decodifica fora da thread principal e desenha só o quadro mais novo (quadros que chegam no meio são pulados)
function draw(tile: Tile, blob: Blob) {
  if (tile.decoding) {
    tile.pending = blob;
    return;
  }
  tile.decoding = true;
  createImageBitmap(blob)
    .then((bmp) => {
      if (tile.canvas.width !== bmp.width || tile.canvas.height !== bmp.height) {
        tile.canvas.width = bmp.width;
        tile.canvas.height = bmp.height;
      }
      tile.ctx.drawImage(bmp, 0, 0);
      bmp.close();
      if (!tile.hasFrame) {
        tile.hasFrame = true;
        tile.msg.classList.add("hidden");
      }
    })
    .catch(() => {})
    .finally(() => {
      tile.decoding = false;
      const next = tile.pending;
      tile.pending = null;
      if (next) draw(tile, next);
    });
}

function renderTiles() {
  screens.classList.toggle("hidden", panelOpen);
  screens.classList.toggle("rm-focus", focus > 0);
  screens.dataset.count = String(tiles.length);
  for (const t of tiles) {
    t.root.classList.toggle("hidden", focus > 0 && t.slot !== focus);
    t.root.classList.toggle("selected", t.slot === selected);
  }
  for (const b of document.querySelectorAll<HTMLButtonElement>("#view-switch .btn")) {
    const f = Number(b.dataset.focus);
    b.classList.toggle("active", f === focus);
    b.classList.toggle("hidden", f > tiles.length);
  }
  panelBtn.classList.toggle("active", panelOpen);
}

function renderStatus() {
  if (!info) return;
  let total = 0;
  for (const t of tiles) {
    const st = info.accounts.find((a) => a.slot === t.slot);
    t.name.textContent = accountName(t.slot);
    if (!st) continue;
    total += st.memMb ?? 0;
    const mem = st.memMb != null ? `${st.memMb} MB` : "";
    const [cls, label] =
      st.state === "running"
        ? [st.memMb != null && st.memMb > info.memAccountMb ? "warn" : "ok", st.popup ? `janela · ${mem}` : mem || "ok"]
        : st.state === "starting"
          ? ["neutral", "abrindo…"]
          : ["bad", "reiniciando…"];
    t.badge.className = `fp-status ${cls}`;
    t.badge.textContent = label;
    const tips = [
      `Recargas: ${st.reloads} · reinícios: ${st.restarts}`,
      st.lastRecycle ? `Vigia: ${st.lastRecycle} (${new Date(st.lastRecycleAt).toLocaleTimeString("pt-BR")})` : "",
      st.error ? `Último erro: ${st.error}` : "",
      st.url,
    ];
    t.badge.title = tips.filter(Boolean).join("\n");
    if (!t.hasFrame) t.msg.textContent = st.state === "running" ? "Carregando o jogo…" : label;
  }
  const gb = (mb: number) => (mb / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 });
  ramTag.textContent = `RAM ${gb(total)} / ${gb(info.memTotalMb)} GB`;
  ramTag.classList.toggle("rm-warn", total > info.memTotalMb);
}

async function loadInfo() {
  try {
    const first = !info;
    info = await api<Info>("/api/info");
    if (first || tiles.length !== info.accounts.length) {
      buildTiles(info.accounts.length);
      screens.style.setProperty("--rm-ratio", `${info.width} / ${info.height}`);
      farm.namesChanged();
      sendView();
    }
    renderStatus();
  } catch (err) {
    console.warn("info:", err);
  }
}

// ---------- WebSocket ----------
function sendJson(msg: object) {
  if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
}

/// Diz ao servidor o que está visível: com o painel aberto ou a aba escondida nada transmite
function sendView() {
  const visible = !panelOpen && document.visibilityState === "visible";
  const slots = !visible ? [] : focus > 0 ? [focus] : tiles.map((t) => t.slot);
  sendJson({ t: "view", slots, focus: visible ? focus : 0 });
}

function connect() {
  const url = `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`;
  const sock = new WebSocket(url);
  sock.binaryType = "arraybuffer";
  ws = sock;
  sock.addEventListener("open", () => {
    retry = 0;
    linkTag.textContent = "online";
    linkTag.classList.remove("rm-off");
    sendView();
  });
  sock.addEventListener("message", (ev) => {
    if (!(ev.data instanceof ArrayBuffer)) return;
    const bytes = new Uint8Array(ev.data);
    const tile = tiles.find((t) => t.slot === bytes[0]);
    if (tile) draw(tile, new Blob([bytes.subarray(1)], { type: "image/jpeg" }));
  });
  sock.addEventListener("close", () => {
    if (ws !== sock) return;
    ws = null;
    linkTag.textContent = "offline";
    linkTag.classList.add("rm-off");
    retry = Math.min(retry + 1, 5);
    window.setTimeout(connect, 1000 * 2 ** retry);
  });
}

// ---------- entrada ----------
/// Posição (0..1) do ponteiro sobre a imagem, descontando as faixas pretas do `object-fit: contain`
function pos(tile: Tile, ev: { clientX: number; clientY: number }): [number, number] | null {
  const r = tile.canvas.getBoundingClientRect();
  const w = tile.canvas.width || info?.width || 16;
  const hh = tile.canvas.height || info?.height || 9;
  const scale = Math.min(r.width / w, r.height / hh);
  const dw = w * scale;
  const dh = hh * scale;
  const x = (ev.clientX - r.left - (r.width - dw) / 2) / dw;
  const y = (ev.clientY - r.top - (r.height - dh) / 2) / dh;
  return x < 0 || x > 1 || y < 0 || y > 1 ? null : [x, y];
}

function select(slot: number) {
  if (selected === slot) return;
  selected = slot;
  renderTiles();
}

function bindInput(tile: Tile) {
  const c = tile.canvas;
  let lastMove = 0;
  let moveTimer: number | undefined;
  let held = false;
  // Toque: só vira clique ao soltar (se não arrastou); arrastar rola a página do jogo
  let touch: { x: number; y: number; lastY: number; lastX: number; scrolling: boolean } | null = null;

  c.addEventListener("contextmenu", (e) => e.preventDefault());

  c.addEventListener("pointerdown", (e) => {
    select(tile.slot);
    const p = pos(tile, e);
    if (!p) return;
    e.preventDefault();
    c.setPointerCapture(e.pointerId);
    if (e.pointerType === "touch") {
      touch = { x: e.clientX, y: e.clientY, lastX: e.clientX, lastY: e.clientY, scrolling: false };
      return;
    }
    held = true;
    sendJson({ t: "mouse", s: tile.slot, k: "down", x: p[0], y: p[1], b: e.button, c: Math.max(1, e.detail) });
  });

  c.addEventListener("pointermove", (e) => {
    if (touch) {
      if (!touch.scrolling && Math.hypot(e.clientX - touch.x, e.clientY - touch.y) > TAP_SLOP) touch.scrolling = true;
      // Na grade do celular, arrastar rola a lista de contas (o navegador cuida); ampliada, rola o jogo
      if (touch.scrolling && focus === tile.slot) {
        const p = pos(tile, e);
        const dy = (touch.lastY - e.clientY) * 2;
        const dx = (touch.lastX - e.clientX) * 2;
        touch.lastX = e.clientX;
        touch.lastY = e.clientY;
        if (p && (dx || dy)) sendJson({ t: "wheel", s: tile.slot, x: p[0], y: p[1], dx, dy });
      }
      return;
    }
    // Passar o mouse por cima só conta com a conta ampliada (hover do jogo); na grade, só arrastando
    if (!held && focus !== tile.slot) return;
    const send = () => {
      lastMove = performance.now();
      const p = pos(tile, e);
      if (p) sendJson({ t: "mouse", s: tile.slot, k: "move", x: p[0], y: p[1], b: e.button < 0 ? 0 : e.button });
    };
    window.clearTimeout(moveTimer);
    const wait = MOVE_MS - (performance.now() - lastMove);
    if (wait <= 0) send();
    else moveTimer = window.setTimeout(send, wait);
  });

  const release = (e: PointerEvent) => {
    const p = pos(tile, e);
    if (touch) {
      const tap = !touch.scrolling;
      touch = null;
      if (tap && p) {
        sendJson({ t: "mouse", s: tile.slot, k: "down", x: p[0], y: p[1], b: 0, c: 1 });
        sendJson({ t: "mouse", s: tile.slot, k: "up", x: p[0], y: p[1], b: 0, c: 1 });
      }
      return;
    }
    if (!held) return;
    held = false;
    window.clearTimeout(moveTimer);
    const [x, y] = p ?? [0, 0];
    sendJson({ t: "mouse", s: tile.slot, k: "up", x, y, b: e.button, c: Math.max(1, e.detail) });
  };
  c.addEventListener("pointerup", release);
  // O navegador assumiu o gesto (rolagem): não é clique
  c.addEventListener("pointercancel", (e) => {
    if (touch) touch = null;
    else release(e);
  });

  c.addEventListener(
    "wheel",
    (e) => {
      const p = pos(tile, e);
      if (!p) return;
      e.preventDefault();
      const unit = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 400 : 1;
      sendJson({ t: "wheel", s: tile.slot, x: p[0], y: p[1], dx: e.deltaX * unit, dy: e.deltaY * unit });
    },
    { passive: false },
  );

  c.addEventListener("dblclick", (e) => {
    // Duplo clique na grade com Alt amplia a conta (sem Alt, o duplo clique vai para o jogo)
    if (e.altKey) setFocus(focus === tile.slot ? 0 : tile.slot);
  });
}

const mods = (e: KeyboardEvent) => (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0);

/// Atalhos que ficam com o navegador de quem está olhando (recarregar, fechar aba, colar etc.)
function browserShortcut(e: KeyboardEvent): boolean {
  if (e.key === "F5" || e.key === "F11" || e.key === "F12") return true;
  return (e.ctrlKey || e.metaKey) && ["r", "t", "w", "n", "l", "v", "Tab"].includes(e.key.length === 1 ? e.key.toLowerCase() : e.key);
}

function sendKey(e: KeyboardEvent, kind: "down" | "up") {
  if (e.key === "Dead" || e.key === "Process" || e.key === "Unidentified") return;
  sendJson({ t: "key", s: selected, k: kind, key: e.key, code: e.code, m: mods(e) });
}

function keyTarget(e: KeyboardEvent): boolean {
  if (panelOpen || e.target === kbdSink) return false;
  const el = e.target as HTMLElement | null;
  return !(el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable));
}

document.addEventListener("keydown", (e) => {
  if (!keyTarget(e) || browserShortcut(e)) return;
  e.preventDefault();
  sendKey(e, "down");
});
document.addEventListener("keyup", (e) => {
  if (!keyTarget(e) || browserShortcut(e)) return;
  e.preventDefault();
  sendKey(e, "up");
});
document.addEventListener("paste", (e) => {
  if (panelOpen) return;
  const text = e.clipboardData?.getData("text/plain");
  if (text) {
    e.preventDefault();
    sendJson({ t: "text", s: selected, text });
  }
});

// Teclado do celular: o texto chega pelo evento input (teclados virtuais mandam keyCode 229); teclas especiais, pelo keydown
kbdSink.addEventListener("input", () => {
  if (kbdSink.value) sendJson({ t: "text", s: selected, text: kbdSink.value });
  kbdSink.value = "";
});
kbdSink.addEventListener("keydown", (e) => {
  if (["Enter", "Backspace", "Tab", "Escape", "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
    e.preventDefault();
    sendKey(e, "down");
    sendKey(e, "up");
  }
});

// ---------- controles ----------
function setFocus(f: number) {
  focus = f;
  if (f > 0) selected = f;
  if (panelOpen) setPanel(false);
  renderTiles();
  sendView();
}

function setPanel(open: boolean) {
  panelOpen = open;
  if (open) farm.show();
  else farm.hide();
  renderTiles();
  sendView();
}

const farm = initFarmPanel({
  names,
  openAccount: (slot) => setFocus(slot),
  api: farmApi,
});

for (const b of document.querySelectorAll<HTMLButtonElement>("#view-switch .btn")) {
  b.addEventListener("click", () => {
    const f = Number(b.dataset.focus);
    setFocus(f === focus && f > 0 ? 0 : f);
  });
}
panelBtn.addEventListener("click", () => setPanel(!panelOpen));
$("btn-kbd").addEventListener("click", () => {
  kbdSink.value = "";
  kbdSink.focus();
});
$("btn-reload").addEventListener("click", async () => {
  if (!confirm(`Recarregar ${accountName(selected)} na VPS?`)) return;
  try {
    await api<void>(`/api/reload/${selected}`, { method: "POST" });
  } catch (err) {
    alert(`Erro ao recarregar: ${err}`);
  }
});
document.addEventListener("visibilitychange", sendView);

void loadInfo().then(connect);
window.setInterval(() => {
  if (document.visibilityState === "visible") void loadInfo();
}, INFO_MS);
