// Painel do Modo Eco: o que cada conta está farmando, lido do WebSocket do jogo pelo lado nativo (src-tauri/src/farm.rs
// no app, server/ na versão remota). Nada aqui toca nas contas. Nomes vêm do jogo: tudo entra por textContent
// (helper `h`), nunca como HTML; o único innerHTML é o esqueleto fixo de farm-panel.html.
import { invoke } from "@tauri-apps/api/core";
import markup from "./farm-panel.html?raw";
import { h, ivColor, qualityTier, save, store } from "./tools/ui";

interface Totals {
  gold: number;
  xp: number;
  kills: number;
  caps: number;
  shinySeen: number;
  shinyCaught: number;
}

type Status = "waiting" | "offline" | "fainted" | "stalled" | "balls" | "hunting" | "city";

interface AccountView {
  slot: number;
  status: Status;
  statusMin: number;
  pokemon: string;
  level: number;
  inHunt: boolean;
  huntSlug: string;
  huntName: string;
  elapsedMs: number;
  gold: number;
  xp: number;
  kills: number;
  caps: number;
  balls: number;
  shinySeen: number;
  ballsLeft: number | null;
}

interface CatchEntry {
  account: number;
  name: string;
  speciesId: number;
  iv: number;
  q: number;
  shiny: boolean;
  new: boolean;
  balls: number;
  t: number;
}

interface ShinyView {
  account: number;
  name: string;
  speciesId: number;
  t: number;
  captured: boolean;
  pending: boolean;
}

interface DropRow {
  a: number;
  id: number;
  name: string;
  qty: number;
  gold: number;
}

export interface Snapshot {
  now: number;
  today: Totals;
  week: Totals;
  quarter: Totals;
  accounts: AccountView[];
  catches: CatchEntry[];
  shinies: ShinyView[];
  shinySession: [number, number, number];
  dropsToday: DropRow[];
  huntsCount: number;
}

/// De onde vêm os dados: comandos do Tauri no app, HTTP na página remota
export interface FarmApi {
  snapshot(tzOffsetMin: number): Promise<Snapshot>;
  reset(): Promise<void>;
  /// Gera a planilha e devolve a mensagem para o aviso
  exportCsv(kind: "days" | "hunts", tzOffsetMin: number, names: string[]): Promise<string>;
}

const tauriApi: FarmApi = {
  snapshot: (tzOffsetMin) => invoke<Snapshot>("farm_snapshot", { tzOffsetMin }),
  reset: () => invoke("farm_reset"),
  exportCsv: async (kind, tzOffsetMin, names) =>
    `Planilha salva em ${await invoke<string>("farm_export", { kind, tzOffsetMin, names })}`,
};

export interface FarmPanelOptions {
  /// Nome de cada conta (índice 0 = conta 1); vazio usa "Conta N"
  names: () => string[];
  /// Clique numa linha da tabela: sair do Modo Eco e ampliar aquela conta
  openAccount: (slot: number) => void;
  /// Padrão: comandos do Tauri
  api?: FarmApi;
}

const POLL_MS = 3000;
const SLOT_COLORS = ["#3b82f6", "#10b981", "#f59e0b", "#a855f7"];
const HOUR = 3_600_000;

// ---------- formatação ----------
const nf0 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
const nf1 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 1 });
const nf2 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 2 });
const nfq = new Intl.NumberFormat("pt-BR", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/// 1.930.000.000 -> "1,93B"; 52.800 -> "52,8K"; com `signed`, "+6M"
function kmb(n: number, signed = false): string {
  const a = Math.abs(n);
  const sign = n < 0 && Math.round(a) > 0 ? "-" : signed && n > 0 ? "+" : "";
  const unit = (v: number) => (v >= 100 ? nf0 : v >= 10 ? nf1 : nf2).format(v);
  if (a >= 1e9) return sign + unit(a / 1e9) + "B";
  if (a >= 1e6) return sign + unit(a / 1e6) + "M";
  if (a >= 1e3) return sign + unit(a / 1e3) + "K";
  return sign + nf0.format(a);
}

/// "10M", "500k", "1,5b" ou número puro
function parseKmb(raw: string): number | null {
  const m = raw.trim().replace(/\./g, "").replace(",", ".").match(/^(\d+(?:\.\d+)?)\s*([kmb])?$/i);
  if (!m) return null;
  const mul = { k: 1e3, m: 1e6, b: 1e9 }[(m[2] ?? "").toLowerCase() as "k" | "m" | "b"] ?? 1;
  return Number(m[1]) * mul;
}

function fmtDur(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const hh = Math.floor(s / 3600);
  const mm = Math.floor((s % 3600) / 60);
  const ss = s % 60;
  const p = (v: number) => String(v).padStart(2, "0");
  return hh > 0 ? `${hh}h ${p(mm)}m` : `${mm}m ${p(ss)}s`;
}

const hhmm = (t: number) => new Date(t).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });

function localDate(t: number): string {
  const d = new Date(t);
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function todayStart(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const perHour = (v: number, ms: number) => (ms >= 60_000 ? (v * HOUR) / ms : 0);

// ---------- status ----------
const STATUS_RANK: Record<Status, number> = { offline: 0, fainted: 1, stalled: 2, balls: 3, waiting: 4, city: 5, hunting: 6 };

function statusBadge(a: AccountView): HTMLSpanElement {
  const min = Math.round(a.statusMin);
  const left = min < 60 ? `~${Math.max(1, min)}min` : `~${Math.round(min / 60)}h`;
  const [cls, label] = {
    waiting: ["muted", "Aguardando…"],
    offline: ["bad", "Desconectada"],
    fainted: ["bad", "Time derrotado"],
    stalled: ["warn", `⚠ Parada há ${min} min`],
    balls: ["warn", `⚠ Pokébolas p/ ${left}`],
    hunting: ["ok", "Caçando"],
    city: ["neutral", "Na cidade"],
  }[a.status];
  const title = {
    waiting: "Ainda sem mensagens do jogo (tela de login ou carregando)",
    offline: "O WebSocket do jogo fechou e não voltou",
    fainted: "Time inteiro derrotado: o farm para até o próximo Pokémon entrar",
    stalled: "Na hunt, mas sem abate há mais de 10 minutos",
    balls: "Estimativa pelo ritmo de pokébolas desta sessão",
    hunting: "Na hunt e abatendo",
    city: "Fora da hunt",
  }[a.status];
  return h("span", { class: `fp-status ${cls}`, title }, label);
}

// ---------- melhor catch ----------
type BestMode = "ivq" | "iv" | "q";
const BEST_SCORE: Record<BestMode, (c: CatchEntry) => number> = {
  ivq: (c) => c.iv * c.q,
  iv: (c) => c.iv,
  q: (c) => c.q,
};

function catchLabel(c: CatchEntry, withNumbers: boolean): HTMLSpanElement {
  const [, color] = qualityTier(c.q);
  return h(
    "span",
    { class: "fp-catch" },
    c.shiny ? "✨ " : "",
    h("span", { style: `color:${color}` }, c.name),
    withNumbers ? " " : "",
    withNumbers ? h("span", { style: `color:${ivColor(c.iv / 6)}` }, `${nf0.format(c.iv)}/192`) : "",
    withNumbers ? h("span", { class: "fp-mult", style: `color:${color}` }, ` ×${nfq.format(c.q)}`) : "",
  );
}

// ---------- tabela por conta ----------
type SortKey = "slot" | "pokemon" | "hunt" | "time" | "gold" | "xp" | "kills" | "caps" | "shiny" | "best" | "last" | "status";

interface Row {
  a: AccountView;
  name: string;
  best: CatchEntry | null;
  last: CatchEntry | null;
}

const COLUMNS: [SortKey, string, string?][] = [
  ["slot", "Conta"],
  ["pokemon", "Pokémon", "Líder do time"],
  ["hunt", "Hunt"],
  ["time", "⏱ Tempo", "Duração da sessão na hunt atual (ou até o último abate)"],
  ["gold", "Gold/h", "Estimativa: loot a preço de NPC + venda das capturas − pokébolas"],
  ["xp", "XP/h"],
  ["kills", "Kills/h"],
  ["caps", "Capturas", "Capturas nesta sessão"],
  ["shiny", "✨", "Shinies encontrados nesta sessão"],
  ["best", "Melhor catch", "Melhor captura de hoje"],
  ["last", "Último catch"],
  ["status", "Status"],
];

function sortValue(r: Row, key: SortKey, best: BestMode): number | string {
  const a = r.a;
  switch (key) {
    case "slot":
      return a.slot;
    case "pokemon":
      return a.level;
    case "hunt":
      return a.huntName.toLowerCase();
    case "time":
      return a.elapsedMs;
    case "gold":
      return perHour(a.gold, a.elapsedMs);
    case "xp":
      return perHour(a.xp, a.elapsedMs);
    case "kills":
      return perHour(a.kills, a.elapsedMs);
    case "caps":
      return a.caps;
    case "shiny":
      return a.shinySeen;
    case "best":
      return r.best ? BEST_SCORE[best](r.best) : -1;
    case "last":
      return r.last?.t ?? 0;
    case "status":
      return STATUS_RANK[a.status];
  }
}

export function initFarmPanel(opts: FarmPanelOptions) {
  const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
  const api = opts.api ?? tauriApi;
  const panel = $<HTMLElement>("farm-panel");
  panel.innerHTML = markup;
  const tbody = $<HTMLTableSectionElement>("fp-tbody");
  const tfoot = $<HTMLTableSectionElement>("fp-tfoot");
  const theadRow = $<HTMLTableRowElement>("fp-thead-row");
  const bestSel = $<HTMLSelectElement>("fp-best-mode");
  const goalGold = $<HTMLInputElement>("fp-goal-gold");
  const goalCaps = $<HTMLInputElement>("fp-goal-caps");
  const fAccount = $<HTMLSelectElement>("fp-f-account");
  const fSearch = $<HTMLInputElement>("fp-f-search");
  const fPeriod = $<HTMLSelectElement>("fp-f-period");
  const fIv = $<HTMLInputElement>("fp-f-iv");
  const fQ = $<HTMLInputElement>("fp-f-q");
  const fShiny = $<HTMLInputElement>("fp-f-shiny");
  const dAccount = $<HTMLSelectElement>("fp-d-account");
  const toast = $<HTMLDivElement>("fp-toast");

  let snap: Snapshot | null = null;
  let timer: number | undefined;
  let visible = false;
  let inflight = false;

  // Preferências locais (só deste PC): ordenação, modo do melhor catch, metas e filtros
  let sort = store<{ key: SortKey; dir: 1 | -1 }>("farm.sort", { key: "slot", dir: 1 });
  bestSel.value = store<BestMode>("farm.best", "ivq");
  goalGold.value = store("farm.goalGold", "");
  goalCaps.value = store("farm.goalCaps", "");
  const filters = store("farm.filters", { account: "0", search: "", period: "24h", iv: "", q: "", shiny: false });
  fAccount.value = filters.account;
  fSearch.value = filters.search;
  fPeriod.value = filters.period;
  fIv.value = filters.iv;
  fQ.value = filters.q;
  fShiny.checked = filters.shiny;
  dAccount.value = store("farm.dropsAccount", "0");

  const accountName = (slot: number) => opts.names()[slot - 1]?.trim() || `Conta ${slot}`;
  const dot = (slot: number) => h("span", { class: "fp-dot", style: `background:${SLOT_COLORS[slot - 1] ?? "#8b95a8"}` });

  function fillAccountSelects() {
    for (const sel of [fAccount, dAccount]) {
      const keep = sel.value;
      sel.replaceChildren(
        h("option", { value: "0" }, "todas"),
        ...[1, 2, 3, 4].map((s) => h("option", { value: String(s) }, accountName(s))),
      );
      sel.value = keep || "0";
    }
  }

  // Cabeçalho ordenável
  function renderHead() {
    theadRow.replaceChildren(
      ...COLUMNS.map(([key, label, title]) => {
        const arrow = sort.key === key ? (sort.dir === 1 ? " ▴" : " ▾") : "";
        const th = h("th", { title, class: sort.key === key ? "sorted" : undefined }, label + arrow);
        th.addEventListener("click", () => {
          // Números começam do maior; texto e conta, do menor
          const desc = !["slot", "hunt", "status"].includes(key);
          sort = sort.key === key ? { key, dir: sort.dir === 1 ? -1 : 1 } : { key, dir: desc ? -1 : 1 };
          save("farm.sort", sort);
          renderHead();
          renderTable();
        });
        return th;
      }),
    );
  }

  function goalBar(value: number, goalRaw: string): HTMLElement | null {
    const goal = parseKmb(goalRaw);
    if (!goal || goal <= 0) return null;
    const pct = Math.max(0, Math.min(100, (value / goal) * 100));
    return h(
      "div",
      { class: "fp-goal", title: `${nf0.format((value / goal) * 100)}% da meta de ${kmb(goal)}` },
      h("span", { style: `width:${pct}%` }),
    );
  }

  function renderToday(s: Snapshot) {
    $("fp-date").textContent = localDate(s.now);
    const t = s.today;
    const card = (cls: string, value: string, label: string, extra?: HTMLElement | null, title?: string) =>
      h("div", { class: "fp-stat", title }, h("div", { class: `fp-stat-v ${cls}` }, value), h("div", { class: "fp-stat-l" }, label), extra ?? null);
    $("fp-stats").replaceChildren(
      card(t.gold < 0 ? "neg" : "pos", kmb(t.gold, true), "Gold", goalBar(t.gold, goalGold.value), "Estimativa: loot a preço de NPC + venda das capturas − pokébolas"),
      card("xp", kmb(t.xp), "XP"),
      card("", nf0.format(t.kills), "Kills"),
      card("", nf0.format(t.caps), "Capturas", goalBar(t.caps, goalCaps.value)),
      card("shiny", `${nf0.format(t.shinySeen)} / ${nf0.format(t.shinyCaught)}`, "Shiny enc/cap"),
    );
    const period = (label: string, p: Totals) =>
      h(
        "span",
        {},
        `${label}: `,
        h("b", { class: p.gold < 0 ? "neg" : "pos" }, kmb(p.gold, true)),
        " gold · ",
        h("b", {}, kmb(p.xp)),
        " XP · ",
        h("b", {}, nf0.format(p.caps)),
        " capturas · ",
        h("span", { class: "shiny" }, `✨ ${nf0.format(p.shinySeen)}/${nf0.format(p.shinyCaught)}`),
      );
    $("fp-periods").replaceChildren(period("7 dias", s.week), period("90 dias", s.quarter));
    $("fp-hunts-n").textContent = String(s.huntsCount);
  }

  function rows(s: Snapshot): Row[] {
    const mode = bestSel.value as BestMode;
    const since = todayStart(s.now);
    return s.accounts.map((a) => {
      const mine = s.catches.filter((c) => c.account === a.slot);
      let best: CatchEntry | null = null;
      for (const c of mine) {
        if (c.t >= since && (!best || BEST_SCORE[mode](c) > BEST_SCORE[mode](best))) best = c;
      }
      // `catches` vem do mais novo para o mais velho
      return { a, name: accountName(a.slot), best, last: mine[0] ?? null };
    });
  }

  function renderTable() {
    if (!snap) return;
    const mode = bestSel.value as BestMode;
    const list = rows(snap).sort((x, y) => {
      const a = sortValue(x, sort.key, mode);
      const b = sortValue(y, sort.key, mode);
      const c = typeof a === "string" ? a.localeCompare(String(b)) : a - (b as number);
      return (c || x.a.slot - y.a.slot) * sort.dir;
    });

    tbody.replaceChildren(
      ...list.map(({ a, name, best, last }) => {
        const has = a.elapsedMs >= 60_000 && (a.inHunt || a.kills > 0);
        const rate = (v: number) => (has ? perHour(v, a.elapsedMs) : null);
        const gold = rate(a.gold);
        const tr = h(
          "tr",
          { title: "Abrir esta conta (sai do Modo Eco)" },
          h("td", {}, dot(a.slot), name),
          h("td", {}, a.pokemon ? `${a.pokemon} Lv${nf0.format(a.level)}` : "—"),
          h("td", {}, a.inHunt ? a.huntName || a.huntSlug : "—"),
          h("td", {}, a.inHunt || a.kills > 0 ? fmtDur(a.elapsedMs) : "—"),
          h("td", { class: gold === null ? "" : gold < 0 ? "neg" : "pos" }, gold === null ? "—" : kmb(gold, true)),
          h("td", {}, has ? kmb(rate(a.xp) ?? 0) : "—"),
          h("td", {}, has ? nf0.format(rate(a.kills) ?? 0) : "—"),
          h("td", {}, nf0.format(a.caps)),
          h("td", { class: a.shinySeen > 0 ? "shiny" : "" }, nf0.format(a.shinySeen)),
          h("td", {}, best ? catchLabel(best, true) : "—"),
          h(
            "td",
            {},
            last ? catchLabel(last, false) : "—",
            last ? h("span", { class: "fp-muted" }, ` ${hhmm(last.t)}${last.balls > 0 ? ` · ${last.balls}` : ""}`) : "",
            last && last.balls > 0 ? h("span", { class: "fp-ball", title: "Pokébolas gastas nesta captura" }) : "",
          ),
          h("td", {}, statusBadge(a)),
        );
        tr.addEventListener("click", () => opts.openAccount(a.slot));
        return tr;
      }),
    );

    const active = snap.accounts.filter((a) => a.elapsedMs >= 60_000 && (a.inHunt || a.kills > 0));
    const sum = (f: (a: AccountView) => number) => active.reduce((acc, a) => acc + perHour(f(a), a.elapsedMs), 0);
    const goldSum = sum((a) => a.gold);
    tfoot.replaceChildren(
      h(
        "tr",
        {},
        h("td", {}, `Total (${snap.accounts.filter((a) => a.inHunt).length})`),
        h("td", {}),
        h("td", {}),
        h("td", {}),
        h("td", { class: goldSum < 0 ? "neg" : "pos" }, kmb(goldSum, true)),
        h("td", {}, kmb(sum((a) => a.xp))),
        h("td", {}, nf0.format(sum((a) => a.kills))),
        h("td", {}, nf0.format(snap.accounts.reduce((acc, a) => acc + a.caps, 0))),
        h("td", {}, nf0.format(snap.accounts.reduce((acc, a) => acc + a.shinySeen, 0))),
        h("td", {}),
        h("td", {}),
        h("td", {}),
      ),
    );
  }

  function renderCatches() {
    if (!snap) return;
    const acc = Number(fAccount.value);
    const q = fSearch.value.trim().toLowerCase();
    const span = { "1h": HOUR, "24h": 24 * HOUR, "7d": 7 * 24 * HOUR, all: Infinity }[fPeriod.value] ?? Infinity;
    const minIv = Number(fIv.value.replace(",", ".")) || 0;
    const minQ = Number(fQ.value.replace(",", ".")) || 0;
    const list = snap.catches
      .filter(
        (c) =>
          (!acc || c.account === acc) &&
          (!q || c.name.toLowerCase().includes(q)) &&
          snap!.now - c.t <= span &&
          c.iv >= minIv &&
          c.q >= minQ &&
          (!fShiny.checked || c.shiny),
      )
      .slice(0, 100);
    const box = $("fp-catches");
    if (!list.length) {
      box.replaceChildren(h("p", { class: "fp-empty" }, snap.catches.length ? "Nenhuma captura com esses filtros." : "Nenhuma captura registrada ainda."));
      return;
    }
    box.replaceChildren(
      ...list.map((c) => {
        const [tier, color] = qualityTier(c.q);
        return h(
          "div",
          { class: "fp-li" },
          dot(c.account),
          h(
            "span",
            { class: "fp-li-main" },
            c.new ? h("span", { class: "fp-new", title: "Primeiro desta espécie na caixa da conta" }, "NOVO") : "",
            c.shiny ? "✨ " : "",
            h("b", {}, c.name),
            " ",
            h("span", { style: `color:${color}` }, tier),
            h("span", { class: "fp-muted" }, ` · ${accountName(c.account)}`),
          ),
          h("span", { class: "fp-num", style: `color:${ivColor(c.iv / 6)}` }, `${nf0.format(c.iv)}/192`),
          h("span", { class: "fp-num", style: `color:${color}` }, `×${nfq.format(c.q)}`),
          h("span", { class: "fp-time" }, hhmm(c.t)),
        );
      }),
    );
  }

  function renderShinies() {
    if (!snap) return;
    const [caught, seen, lost] = snap.shinySession;
    $("fp-shiny-counts").replaceChildren(
      h("span", { class: "pos" }, `${caught} capturados`),
      h("span", { class: "warn" }, `${seen} apareceram`),
      h("span", { class: "neg" }, `${lost} perdidos`),
    );
    const box = $("fp-shinies");
    if (!snap.shinies.length) {
      box.replaceChildren(h("p", { class: "fp-empty" }, "Nenhum shiny visto ainda."));
      return;
    }
    box.replaceChildren(
      ...snap.shinies.map((s) => {
        const mark = s.captured
          ? h("span", { class: "pos", title: "Capturado" }, "✓")
          : s.pending
            ? h("span", { class: "fp-muted", title: "Derrotado agora: ainda pode ser capturado" }, "…")
            : h("span", { class: "neg", title: "Derrotado sem captura" }, "✕");
        return h(
          "div",
          { class: "fp-li" },
          dot(s.account),
          h("span", { class: "fp-li-main" }, h("b", { class: "shiny" }, `✨ ${s.name || "?"}`), " ", mark, h("span", { class: "fp-muted" }, ` · ${accountName(s.account)}`)),
          h("span", { class: "fp-time" }, hhmm(s.t)),
        );
      }),
    );
  }

  function renderDrops() {
    if (!snap) return;
    const acc = Number(dAccount.value);
    const merged = new Map<number, DropRow>();
    for (const d of snap.dropsToday) {
      if (acc && d.a !== acc) continue;
      const m = merged.get(d.id);
      if (m) {
        m.qty += d.qty;
        m.gold += d.gold;
      } else merged.set(d.id, { ...d });
    }
    const list = [...merged.values()].sort((a, b) => b.gold - a.gold || b.qty - a.qty);
    const total = list.reduce((s, d) => s + d.gold, 0);
    $("fp-drops-total").textContent = list.length ? `${kmb(total)} gold em ${list.length} itens` : "";
    const box = $("fp-drops");
    if (!list.length) {
      box.replaceChildren(h("p", { class: "fp-empty" }, "Nenhum drop hoje ainda."));
      return;
    }
    box.replaceChildren(
      ...list.slice(0, 60).map((d) =>
        h(
          "div",
          { class: "fp-li" },
          h("span", { class: "fp-li-main" }, d.name || `Item ${d.id}`),
          h("span", { class: "fp-num" }, `×${nf0.format(d.qty)}`),
          h("span", { class: "fp-num gold", title: d.gold > 0 ? "Preço de NPC" : "Sem preço de NPC no catálogo" }, d.gold > 0 ? kmb(d.gold) : "—"),
        ),
      ),
    );
  }

  function renderAll() {
    if (!snap) return;
    renderToday(snap);
    renderTable();
    renderCatches();
    renderShinies();
    renderDrops();
  }

  async function refresh() {
    if (inflight) return;
    inflight = true;
    try {
      snap = await api.snapshot(new Date().getTimezoneOffset());
      renderAll();
    } catch (err) {
      console.warn("farm snapshot error:", err);
    } finally {
      inflight = false;
    }
  }

  function loop() {
    window.clearTimeout(timer);
    if (!visible) return;
    void refresh().finally(() => {
      if (visible) timer = window.setTimeout(loop, POLL_MS);
    });
  }

  function showToast(text: string) {
    toast.textContent = text;
    toast.classList.remove("hidden");
    window.clearTimeout(Number(toast.dataset.t));
    toast.dataset.t = String(window.setTimeout(() => toast.classList.add("hidden"), 6000));
  }

  async function exportCsv(kind: "days" | "hunts") {
    try {
      const names = [1, 2, 3, 4].map(accountName);
      showToast(await api.exportCsv(kind, new Date().getTimezoneOffset(), names));
    } catch (err) {
      showToast(`Erro ao exportar: ${err}`);
    }
  }

  // ---------- eventos ----------
  bestSel.addEventListener("change", () => {
    save("farm.best", bestSel.value);
    renderTable();
  });
  for (const [input, key] of [
    [goalGold, "farm.goalGold"],
    [goalCaps, "farm.goalCaps"],
  ] as const) {
    input.addEventListener("input", () => {
      save(key, input.value);
      if (snap) renderToday(snap);
    });
  }
  const saveFilters = () => {
    save("farm.filters", {
      account: fAccount.value,
      search: fSearch.value,
      period: fPeriod.value,
      iv: fIv.value,
      q: fQ.value,
      shiny: fShiny.checked,
    });
    renderCatches();
  };
  for (const el of [fAccount, fSearch, fPeriod, fIv, fQ, fShiny]) el.addEventListener("input", saveFilters);
  dAccount.addEventListener("input", () => {
    save("farm.dropsAccount", dAccount.value);
    renderDrops();
  });
  $("fp-reset").addEventListener("click", async () => {
    try {
      await api.reset();
      await refresh();
    } catch (err) {
      console.warn("farm reset error:", err);
    }
  });
  $("fp-export-days").addEventListener("click", () => void exportCsv("days"));
  $("fp-export-hunts").addEventListener("click", () => void exportCsv("hunts"));

  fillAccountSelects();
  renderHead();

  return {
    show() {
      visible = true;
      panel.classList.remove("hidden");
      fillAccountSelects();
      loop();
    },
    hide() {
      visible = false;
      window.clearTimeout(timer);
      panel.classList.add("hidden");
    },
    /// Os nomes das contas mudaram (modal Contas)
    namesChanged() {
      fillAccountSelects();
      renderAll();
    },
  };
}
