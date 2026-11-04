import { invoke } from "@tauri-apps/api/core";
import {
  STAT_LABELS, TYPES, areaTag, debounce, fmt, fmt2, fmtCompact, fmtMult, h, ivColor, num, qualityTier, save,
  sprite, store, tierOf, typeChip, typeName,
} from "./ui";

// ---- Tipos que vêm do Rust (serde camelCase) ----
interface SpeciesBrief { name: string; id: number; types: string[] }
interface CatalogInfo { fetchedAt: number; hunts: number; species: SpeciesBrief[]; warning: string | null }
interface Move { name: string; power: number; ty: string; physical: boolean; cooldownMs: number; learnLevel: number; tm: string }
interface Species { id: number; name: string; types: string[]; bases: number[]; huntLevel: number; minLevel: number; isFinal: boolean; moves: Move[] }
interface IvResult { values: number[]; ranges: ([number, number] | null)[]; source: string; exact: boolean; quality: number | null }
interface Projection { stats: number[]; power: number; quality: number }
interface Analysis {
  species: Species; iv: IvResult; current: Projection | null; target: Projection | null; maxStats: number[] | null;
  matchups: { defense: [string, number][]; offense: [string, [string, number][]][] };
}
interface Sug { ritmo: number; golpes: number; killsH: number; xpH: number; goldH: number; move: string; eff: number; physical: boolean; folga: number; tm: string }
interface HuntRef { id: number; name: string; level: number; area: string; types: string[] }
interface TierRow { species: string; base: string; id: number; types: string[]; score: number; total: number; totalTm: string; sug: Sug; hunt: HuntRef; outland: [HuntRef, number] | null }
interface HuntRow { hunt: HuntRef; species: string; id: number; score: number; sug: Sug }

/// O que o usuário digitou no card de IV (é o que fica salvo nos fixados)
interface IvForm { species: string; level: string; quality: string; total: string; power: string; stats: string[]; target: string }

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let speciesNames = new Map<string, SpeciesBrief>();

// ================= Dados do jogo =================

const dataInfo = $("data-info");
const dataWarning = $("data-warning");
const btnRefresh = $<HTMLButtonElement>("btn-refresh");

async function loadCatalog(force: boolean) {
  btnRefresh.disabled = true;
  dataInfo.textContent = force ? "Baixando dados do jogo…" : "Carregando dados do jogo…";
  try {
    const info = await invoke<CatalogInfo>("tools_catalog", { force });
    speciesNames = new Map(info.species.map((s) => [s.name.toLowerCase(), s]));
    $("species-list").replaceChildren(...info.species.map((s) => h("option", { value: s.name })));
    const when = new Date(info.fetchedAt * 1000).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });
    dataInfo.textContent = `${info.species.length} espécies · ${info.hunts} hunts · dados de ${when}`;
    dataWarning.textContent = info.warning ?? "";
    dataWarning.classList.toggle("hidden", !info.warning);
    tierCache.clear();
    refreshActive();
  } catch (err) {
    dataInfo.textContent = "Sem dados do jogo";
    dataWarning.textContent = String(err);
    dataWarning.classList.remove("hidden");
  } finally {
    btnRefresh.disabled = false;
  }
}

btnRefresh.addEventListener("click", () => loadCatalog(true));

// ================= Abas =================

type Tab = "iv" | "tier" | "hunt";
let activeTab: Tab = store<Tab>("pg-tools-tab", "iv");

function showTab(tab: Tab) {
  activeTab = tab;
  save("pg-tools-tab", tab);
  document.querySelectorAll<HTMLButtonElement>(".tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab));
  document.querySelectorAll<HTMLElement>(".tab-panel").forEach((p) => p.classList.toggle("hidden", p.id !== `tab-${tab}`));
  refreshActive();
}

function refreshActive() {
  if (!speciesNames.size) return;
  if (activeTab === "iv") void analyze();
  else if (activeTab === "tier") void renderTier();
  else void renderHunts();
}

document.querySelectorAll<HTMLButtonElement>(".tab").forEach((b) =>
  b.addEventListener("click", () => showTab(b.dataset.tab as Tab)),
);

/// Segmentado simples: devolve o valor ativo e chama `onChange` ao trocar
function segmented(id: string, attr: string, initial: string, onChange: (v: string) => void) {
  const root = $(id);
  let value = initial;
  const render = () =>
    root.querySelectorAll<HTMLButtonElement>("button").forEach((b) => b.classList.toggle("active", b.dataset[attr] === value));
  root.querySelectorAll<HTMLButtonElement>("button").forEach((b) =>
    b.addEventListener("click", () => {
      value = b.dataset[attr] ?? value;
      render();
      onChange(value);
    }),
  );
  render();
  return { get: () => value, set: (v: string) => { value = v; render(); } };
}

/// Chips de tipo (com "Geral" opcional na frente)
function typeChips(id: string, initial: string, withAll: boolean, onChange: (v: string) => void) {
  const root = $(id);
  let value = initial;
  const options = (withAll ? ["ALL"] : []).concat(TYPES as unknown as string[]);
  const buttons = options.map((t) => {
    const b = h("button", { class: "chip", "data-type": t }, t === "ALL" ? "Geral" : typeName(t));
    b.addEventListener("click", () => {
      value = t;
      render();
      onChange(t);
    });
    return b;
  });
  const render = () => buttons.forEach((b) => b.classList.toggle("active", b.dataset.type === value));
  root.replaceChildren(...buttons);
  render();
  return { get: () => value };
}

// ================= Qualidade / IV =================

const ivSpecies = $<HTMLInputElement>("iv-species");
const ivLevel = $<HTMLInputElement>("iv-level");
const ivQuality = $<HTMLInputElement>("iv-quality");
const ivTotal = $<HTMLInputElement>("iv-total");
const ivPower = $<HTMLInputElement>("iv-power");
const ivTarget = $<HTMLInputElement>("iv-target");
const statInputs = [...document.querySelectorAll<HTMLInputElement>("[data-stat]")];
const ivResult = $("iv-result");

function readForm(): IvForm {
  return {
    species: ivSpecies.value.trim(),
    level: ivLevel.value,
    quality: ivQuality.value,
    total: ivTotal.value,
    power: ivPower.value,
    stats: statInputs.map((i) => i.value),
    target: ivTarget.value,
  };
}

function writeForm(f: IvForm) {
  ivSpecies.value = f.species;
  ivLevel.value = f.level;
  ivQuality.value = f.quality;
  ivTotal.value = f.total;
  ivPower.value = f.power;
  statInputs.forEach((i, k) => (i.value = f.stats[k] ?? ""));
  ivTarget.value = f.target;
}

const parse = (s: string) => {
  const v = Number(s.trim().replace(",", "."));
  return s.trim() && Number.isFinite(v) ? v : null;
};

/// Pede a análise ao Rust; null quando o formulário ainda não tem espécie, nível e qualidade
async function analyzeForm(f: IvForm): Promise<Analysis | null> {
  const level = parse(f.level);
  const quality = parse(f.quality);
  if (!speciesNames.has(f.species.toLowerCase()) || !level || !quality) return null;
  const decimals = (f.quality.trim().replace(",", ".").split(".")[1] ?? "").length;
  return invoke<Analysis>("tools_analyze", {
    input: {
      species: f.species,
      level,
      quality,
      qualityDecimals: decimals,
      ivTotal: parse(f.total),
      power: parse(f.power),
      stats: f.stats.map(parse),
      targetLevel: parse(f.target),
    },
  });
}

const IV_SOURCE: Record<string, string> = {
  "observed-exact": "IVs exatos pelos atributos",
  "observed-range": "Faixa de IV: em nível baixo dois IVs podem dar o mesmo atributo",
  "total-scenario": "Sem os 6 atributos: cenário distribuindo o IV total por igual",
  unknown: "Preencha os 6 atributos (ou o IV total) para ler os IVs",
};

let lastAnalysis: Analysis | null = null;

const analyze = debounce(async () => {
  save("pg-tools-iv", readForm());
  let a: Analysis | null = null;
  try {
    a = await analyzeForm(readForm());
  } catch (err) {
    ivResult.replaceChildren(h("div", { class: "card empty-state" }, String(err)));
    return;
  }
  lastAnalysis = a;
  if (!a) {
    ivResult.replaceChildren(
      h("div", { class: "card empty-state" },
        "Escolha a espécie e preencha nível, qualidade e os atributos que o jogo mostra. Os IVs saem pela fórmula do jogo."),
    );
  } else {
    ivResult.replaceChildren(renderAnalysis(a, readForm()));
  }
  void renderCompare();
}, 150);

function ivText(iv: IvResult, k: number): string {
  const r = iv.ranges[k];
  if (iv.source === "unknown") return "—";
  return r && r[0] !== r[1] ? `${r[0]}-${r[1]}` : String(iv.values[k]);
}

function renderAnalysis(a: Analysis, f: IvForm): HTMLElement {
  const { species: sp, iv, current, target, maxStats } = a;
  const quality = iv.quality ?? parse(f.quality) ?? 1;
  const [tierName, tierColor] = qualityTier(quality);
  const ivSum = iv.values.reduce((s, v) => s + v, 0);
  const known = iv.source !== "unknown";
  const level = parse(f.level) ?? 0;

  // melhor e pior atributo pelo IV
  const best = known ? iv.values.indexOf(Math.max(...iv.values)) : -1;
  const worst = known ? iv.values.indexOf(Math.min(...iv.values)) : -1;

  const head = h("div", { class: "card result-head" },
    sprite(sp.id),
    h("div", { class: "result-title" },
      h("h2", {}, sp.name, h("span", { class: "lv" }, ` Lv ${fmt(level)}`)),
      h("div", { class: "chips-inline" }, ...sp.types.map(typeChip)),
    ),
    h("div", { class: "badges" },
      h("span", { class: "badge", style: `--c:${tierColor}`, title: `Qualidade ${fmt2(quality)}${iv.quality ? " (achada pelos atributos)" : ""}` },
        `${tierName} ×${quality.toFixed(iv.quality ? 3 : 2)}`),
      known && h("span", { class: "badge", title: "Soma dos 6 IVs (máximo 192)" }, `IV ${ivSum}/192 · ${Math.round((ivSum / 192) * 100)}%`),
      current && h("span", { class: "badge", title: "Poder no nível atual" }, `Poder ${fmt(current.power)}`),
    ),
  );

  const rows = STAT_LABELS.map((label, k) => {
    const v = iv.values[k];
    const cur = current?.stats[k];
    const max = maxStats?.[k];
    const gain = cur !== undefined && max !== undefined ? max - cur : null;
    return h("tr", { class: k === best ? "best" : k === worst ? "worst" : "" },
      h("td", {}, label, k === best ? " ▲" : k === worst ? " ▼" : ""),
      h("td", { class: "muted" }, fmt(sp.bases[k])),
      h("td", {}, cur !== undefined ? fmt(cur) : "—"),
      h("td", { class: "iv-cell" },
        h("span", { class: "iv-bar" }, h("span", { style: `width:${(v / 32) * 100}%;background:${ivColor(v)}` })),
        h("b", { style: `color:${known ? ivColor(v) : "inherit"}` }, ivText(iv, k)),
      ),
      h("td", { class: "muted", title: "Quanto o atributo subiria com IV 32 neste nível" },
        known && gain !== null ? (gain > 0 ? `+${fmt(gain)} (falta ${32 - v} IV)` : "máximo") : "—"),
      target && h("td", {}, fmt(target.stats[k])),
    );
  });

  const table = h("table", { class: "table stats-table" },
    h("thead", {}, h("tr", {},
      h("th", {}, "Atributo"), h("th", {}, "Base"), h("th", {}, "Atual"), h("th", {}, "IV"), h("th", {}, "Até IV 32"),
      target && h("th", {}, `Lv ${fmt(parse(f.target) ?? 0)}`),
    )),
    h("tbody", {}, ...rows),
    target && h("tfoot", {}, h("tr", {}, h("td", { colspan: 5 }, "Poder projetado"), h("td", {}, fmt(target.power)))),
  );

  const statsCard = h("div", { class: "card" },
    h("p", { class: `source ${iv.exact ? "ok" : ""}` }, IV_SOURCE[iv.source] ?? iv.source),
    h("div", { class: "table-wrap" }, table),
  );

  return h("div", { class: "result-stack" }, head, statsCard, renderMatchups(a), renderMoves(sp, level));
}

function multGroup(title: string, items: [string, number][], cls: string) {
  if (!items.length) return "";
  return h("div", { class: "mult-group" },
    h("span", { class: "mult-title" }, title),
    ...items.map(([t, m]) => h("span", { class: `mult ${cls}` }, typeChip(t), h("b", {}, fmtMult(m)))),
  );
}

function renderMatchups(a: Analysis) {
  const d = a.matchups.defense;
  const card = h("div", { class: "card" },
    h("div", { class: "card-title" }, h("h3", {}, "Efetividade na hunt"),
      h("span", { class: "hint" }, "Regra da hunt: ×2 vira ×2,5, ×4 vira ×5,5 e resistências dividem por 1,5")),
    h("p", { class: "sub" }, "Recebe dano de:"),
    multGroup("Fraco a", d.filter(([, m]) => m > 1), "bad"),
    multGroup("Resiste", d.filter(([, m]) => m > 0 && m < 1), "good"),
    multGroup("Imune", d.filter(([, m]) => m === 0), "good"),
  );
  for (const [t, list] of a.matchups.offense) {
    card.append(
      h("p", { class: "sub" }, "Golpe de ", typeChip(t), " (STAB):"),
      multGroup("Forte contra", list.filter(([, m]) => m > 1), "good"),
      multGroup("Fraco contra", list.filter(([, m]) => m > 0 && m < 1), "bad"),
      multGroup("Não afeta", list.filter(([, m]) => m === 0), "bad"),
    );
  }
  return card;
}

function renderMoves(sp: Species, level: number) {
  const moves = [...sp.moves].sort((p, q) => p.learnLevel - q.learnLevel || q.power - p.power);
  return h("details", { class: "card" },
    h("summary", {}, h("h3", {}, `Golpes (${moves.length})`)),
    h("div", { class: "table-wrap" }, h("table", { class: "table" },
      h("thead", {}, h("tr", {}, h("th", {}, "Lv"), h("th", {}, "Golpe"), h("th", {}, "Tipo"), h("th", {}, "Cat."), h("th", {}, "Poder"), h("th", {}, "Recarga"))),
      h("tbody", {}, ...moves.map((m) =>
        h("tr", { class: m.learnLevel > level ? "dim" : "" },
          h("td", {}, fmt(m.learnLevel)),
          h("td", {}, m.name, m.tm && h("span", { class: "tm-tag" }, "TM")),
          h("td", {}, typeChip(m.ty)),
          h("td", {}, m.power > 0 ? h("span", { class: `cat ${m.physical ? "" : "esp"}` }, m.physical ? "FÍS" : "ESP") : "—"),
          h("td", {}, m.power > 0 ? fmt(m.power) : "—"),
          h("td", { class: "muted" }, m.cooldownMs > 0 ? `${fmt2(m.cooldownMs / 1000)} s` : "—"),
        ),
      )),
    )),
  );
}

// ---- Fixados / Comparar ----

const MAX_PINS = 3;
let pins = store<IvForm[]>("pg-tools-pins", []);

$("btn-pin").addEventListener("click", () => {
  const f = readForm();
  if (!lastAnalysis) return;
  pins = [f, ...pins.filter((p) => JSON.stringify(p) !== JSON.stringify(f))].slice(0, MAX_PINS);
  save("pg-tools-pins", pins);
  void renderCompare();
});

$("btn-iv-clear").addEventListener("click", () => {
  writeForm({ species: "", level: "", quality: "", total: "", power: "", stats: ["", "", "", "", "", ""], target: "" });
  analyze();
});

async function renderCompare() {
  const box = $("compare");
  box.classList.toggle("hidden", !pins.length);
  if (!pins.length) return;
  const cur = lastAnalysis;
  const analyses = await Promise.all(pins.map((p) => analyzeForm(p).catch(() => null)));
  const cols: { label: string; a: Analysis | null; f: IvForm | null; idx: number }[] = [
    ...(cur ? [{ label: "Atual", a: cur, f: readForm(), idx: -1 }] : []),
    ...pins.map((p, i) => ({ label: p.species || "?", a: analyses[i], f: p, idx: i })),
  ];
  const cell = (val: number | undefined, ref: number | undefined, isRef: boolean, text?: string) => {
    if (val === undefined) return h("td", {}, "—");
    const diff = !isRef && ref !== undefined ? val - ref : 0;
    return h("td", { class: diff > 0 ? "up" : diff < 0 ? "down" : "" }, text ?? fmt(val), diff ? h("small", {}, ` ${diff > 0 ? "+" : ""}${fmt(diff)}`) : null);
  };
  const ref = cur;
  const line = (label: string, get: (a: Analysis, f: IvForm) => number | undefined, text?: (a: Analysis, f: IvForm) => string) =>
    h("tr", {}, h("th", {}, label), ...cols.map((c) => {
      if (!c.a || !c.f) return h("td", {}, "—");
      const v = get(c.a, c.f);
      return cell(v, ref ? get(ref, readForm()) : undefined, c.idx === -1 || !ref, text?.(c.a, c.f));
    }));

  const head = h("tr", {}, h("th", {}), ...cols.map((c) => {
    const th = h("th", {}, c.label);
    if (c.idx >= 0) {
      const btnOpen = h("button", { class: "icon-btn", title: "Abrir no card" }, "↺");
      btnOpen.addEventListener("click", () => { writeForm(pins[c.idx]); analyze(); });
      const btnDel = h("button", { class: "icon-btn", title: "Tirar dos fixados" }, "✕");
      btnDel.addEventListener("click", () => { pins.splice(c.idx, 1); save("pg-tools-pins", pins); void renderCompare(); });
      th.append(" ", btnOpen, btnDel);
    }
    return th;
  }));

  $("compare-table").replaceChildren(
    h("thead", {}, head),
    h("tbody", {},
      line("Nível", (_, f) => parse(f.level) ?? undefined),
      line("Qualidade", (a, f) => a.iv.quality ?? parse(f.quality) ?? undefined, (a, f) => fmt2(a.iv.quality ?? parse(f.quality) ?? 0)),
      line("IV total", (a) => (a.iv.source === "unknown" ? undefined : a.iv.values.reduce((s, v) => s + v, 0))),
      line("Poder", (a) => a.current?.power),
      ...STAT_LABELS.map((label, k) => line(`IV ${label}`, (a) => (a.iv.source === "unknown" ? undefined : a.iv.values[k]))),
      ...STAT_LABELS.map((label, k) => line(label, (a) => a.current?.stats[k])),
    ),
  );
}

[ivSpecies, ivLevel, ivQuality, ivTotal, ivPower, ivTarget, ...statInputs].forEach((i) => i.addEventListener("input", () => analyze()));
$<HTMLFormElement>("iv-form").addEventListener("submit", (e) => e.preventDefault());
writeForm(store<IvForm>("pg-tools-iv", { species: "", level: "", quality: "", total: "", power: "", stats: ["", "", "", "", "", ""], target: "500" }));

$("btn-hunt-with").addEventListener("click", () => {
  const f = readForm();
  hpSpecies.value = f.species;
  if (f.level) hpLevel.value = f.level;
  if (f.quality) hpQuality.value = f.quality;
  const sum = lastAnalysis && lastAnalysis.iv.source !== "unknown" ? lastAnalysis.iv.values.reduce((s, v) => s + v, 0) : null;
  hpIv.value = String(sum ?? (f.total || 96));
  huntMode.set("poke");
  applyHuntMode();
  showTab("hunt");
});

// ================= Tierlist =================

const tierLevel = $<HTMLInputElement>("tier-level");
const tierTm = $<HTMLInputElement>("tier-tm");
const tierSearch = $<HTMLInputElement>("tier-search");
const tierList = $("tier-list");
const tierCache = new Map<string, TierRow[]>();
const tierPrefs = store("pg-tools-tier", { level: "100", goal: "xp", tm: false, type: "ALL" });
tierLevel.value = tierPrefs.level;
tierTm.checked = tierPrefs.tm;
const tierGoal = segmented("tier-goal", "goal", tierPrefs.goal, () => renderTier());
const tierType = typeChips("tier-types", tierPrefs.type, true, () => renderTier());
let tierSeq = 0;

function goalValue(s: Sug, gold: boolean) {
  return gold ? s.goldH : s.xpH;
}

function huntLabel(x: HuntRef, nota?: number) {
  return h("span", { class: "hunt-ref" }, areaTag(x.area), x.name, x.level ? ` Lv${fmt(x.level)}` : "", nota !== undefined ? h("b", {}, ` · ${nota}`) : null);
}

function moveLabel(s: Sug, tm = s.tm) {
  return h("span", { class: "move" },
    h("span", { class: `cat ${s.physical ? "" : "esp"}`, title: s.physical ? "Golpe físico: bate na Defesa" : "Golpe especial: bate na Defesa Especial" }, s.physical ? "FÍS" : "ESP"),
    ` ${s.move}`,
    tm && h("b", { class: "tm-tag" }, `TM ${tm}`),
    ` ${fmtMult(s.eff)}`,
    s.folga > 0 && h("span", { class: "muted", title: "Folga: dano do golpe sobre a vida do selvagem (1 = mata de 1 golpe no limite)" }, ` · folga ${fmtMult(Math.min(s.folga, 9.99))}`),
  );
}

async function renderTier() {
  const level = Math.max(0, num(tierLevel) ?? 0);
  const gold = tierGoal.get() === "gold";
  const withTm = tierTm.checked;
  const type = tierType.get();
  save("pg-tools-tier", { level: tierLevel.value, goal: tierGoal.get(), tm: withTm, type });
  const key = `${level}:${withTm}:${gold}`;
  const seq = ++tierSeq;
  let rows = tierCache.get(key);
  if (!rows) {
    tierList.replaceChildren(h("div", { class: "card empty-state" }, "Calculando…"));
    try {
      rows = await invoke<TierRow[]>("tools_tierlist", { level, withTm, gold });
    } catch (err) {
      tierList.replaceChildren(h("div", { class: "card empty-state" }, String(err)));
      return;
    }
    tierCache.set(key, rows);
  }
  if (seq !== tierSeq) return;

  const all = type === "ALL";
  const value = (r: TierRow) => (all ? r.total : r.score);
  const q = tierSearch.value.trim().toLowerCase();
  const filtered = rows
    .filter((r) => all || r.types.includes(type))
    .sort((p, q2) => value(q2) - value(p));
  const max = filtered.length ? value(filtered[0]) : 0;
  const shown = filtered
    .map((r, i) => ({ r, pos: i + 1 }))
    .filter(({ r }) => !q || r.base.toLowerCase().includes(q) || r.species.toLowerCase().includes(q))
    .slice(0, 150);

  if (!shown.length) {
    tierList.replaceChildren(h("div", { class: "card empty-state" }, "Nenhuma espécie para esse filtro."));
    return;
  }
  tierList.replaceChildren(...shown.map(({ r, pos }) => {
    const pct = max > 0 ? Math.round((value(r) / max) * 100) : 0;
    const [tier, color] = tierOf(pct);
    const outPct = r.outland && max > 0 ? Math.round((r.outland[1] / max) * 100) : undefined;
    return h("div", { class: "row" },
      h("span", { class: "pos" }, `${pos}.`),
      h("span", { class: "tier", style: `--c:${color}` }, tier),
      sprite(r.id) ?? h("span", { class: "sprite" }),
      h("div", { class: "row-main" },
        h("div", {}, h("b", {}, r.base), r.base !== r.species && h("span", { class: "muted" }, ` (${r.species})`), " ", ...r.types.map(typeChip)),
        h("div", { class: "row-sub" }, moveLabel(r.sug, all ? r.totalTm : r.sug.tm)),
        h("div", { class: "row-sub" },
          huntLabel(r.hunt, all ? undefined : pct),
          r.outland && h("span", { class: "muted" }, "  |  "),
          r.outland && huntLabel(r.outland[0], all ? undefined : outPct),
          h("span", { class: "muted" }, `  ·  ${fmtCompact(goalValue(r.sug, gold))} ${gold ? "gold" : "XP"}/h · ${fmt(r.sug.killsH)} kills/h`),
        ),
      ),
      h("span", { class: "score", style: `color:${color}`, title: all ? "Rendimento somado em todas as hunts, em relação ao 1º da lista" : "Rendimento na melhor hunt, em relação ao 1º da aba" }, String(pct)),
    );
  }));
}

tierLevel.addEventListener("input", debounce(() => renderTier(), 300));
tierTm.addEventListener("change", () => renderTier());
tierSearch.addEventListener("input", debounce(() => renderTier(), 120));

// ================= Onde caçar =================

const huntLevel = $<HTMLInputElement>("hunt-level");
const huntTm = $<HTMLInputElement>("hunt-tm");
const huntList = $("hunt-list");
const hpSpecies = $<HTMLInputElement>("hp-species");
const hpLevel = $<HTMLInputElement>("hp-level");
const hpQuality = $<HTMLInputElement>("hp-quality");
const hpIv = $<HTMLInputElement>("hp-iv");
const hpAccount = $<HTMLInputElement>("hp-account");
const huntPrefs = store("pg-tools-hunt", { mode: "type", goal: "xp", tm: false, level: "100", type: "FIRE", sp: "", spLevel: "100", q: "1", iv: "96", acc: "0" });
huntLevel.value = huntPrefs.level;
huntTm.checked = huntPrefs.tm;
hpSpecies.value = huntPrefs.sp;
hpLevel.value = huntPrefs.spLevel;
hpQuality.value = huntPrefs.q;
hpIv.value = huntPrefs.iv;
hpAccount.value = huntPrefs.acc;
const huntMode = segmented("hunt-mode", "mode", huntPrefs.mode, () => { applyHuntMode(); renderHunts(); });
const huntGoal = segmented("hunt-goal", "goal", huntPrefs.goal, () => renderHunts());
const huntType = typeChips("hunt-types", huntPrefs.type, false, () => renderHunts());
let huntSeq = 0;

function applyHuntMode() {
  const poke = huntMode.get() === "poke";
  $("hunt-type-controls").classList.toggle("hidden", poke);
  $("hunt-types").classList.toggle("hidden", poke);
  $("hunt-poke-controls").classList.toggle("hidden", !poke);
}
applyHuntMode();

async function renderHunts() {
  const gold = huntGoal.get() === "gold";
  const withTm = huntTm.checked;
  const poke = huntMode.get() === "poke";
  save("pg-tools-hunt", {
    mode: huntMode.get(), goal: huntGoal.get(), tm: withTm, level: huntLevel.value, type: huntType.get(),
    sp: hpSpecies.value, spLevel: hpLevel.value, q: hpQuality.value, iv: hpIv.value, acc: hpAccount.value,
  });
  const seq = ++huntSeq;
  let rows: HuntRow[];
  try {
    if (poke) {
      if (!speciesNames.has(hpSpecies.value.trim().toLowerCase())) {
        huntList.replaceChildren(h("div", { class: "card empty-state" }, "Escolha a espécie do seu pokémon (com nível, qualidade e IV total) para ver as melhores hunts para ele."));
        return;
      }
      if (/ditto/i.test(hpSpecies.value)) {
        huntList.replaceChildren(h("div", { class: "card empty-state" }, "Ditto copia a espécie do alvo: escolha a espécie em que ele vai se transformar."));
        return;
      }
      rows = await invoke<HuntRow[]>("tools_hunts_for_pokemon", {
        input: {
          species: hpSpecies.value.trim(),
          level: num(hpLevel) ?? 1,
          quality: num(hpQuality) ?? 1,
          ivTotal: num(hpIv),
          withTm,
          gold,
          maxHuntLevel: num(hpAccount) ?? 0,
        },
      });
    } else {
      rows = await invoke<HuntRow[]>("tools_hunts_by_type", { element: huntType.get(), level: num(huntLevel) ?? 1, withTm, gold });
    }
  } catch (err) {
    huntList.replaceChildren(h("div", { class: "card empty-state" }, String(err)));
    return;
  }
  if (seq !== huntSeq) return;
  if (!rows.length) {
    huntList.replaceChildren(h("div", { class: "card empty-state" }, poke ? "Nenhuma hunt viável para esse pokémon nesse nível." : "Nenhuma hunt desse elemento até esse nível."));
    return;
  }
  const max = rows[0].score;
  const top = rows[0];
  const summary = h("div", { class: "card answer" },
    h("span", { class: "muted" }, poke ? "Melhor hunt para ele:" : "Cace em:"),
    h("b", {}, ` ${top.hunt.name}${top.hunt.level ? ` Lv${fmt(top.hunt.level)}` : ""}`),
    !poke && h("span", {}, " com ", h("b", {}, top.species)),
    h("span", { class: "muted" }, ` · ${fmtCompact(goalValue(top.sug, gold))} ${gold ? "gold" : "XP"}/h estimados`),
  );
  huntList.replaceChildren(summary, ...rows.slice(0, 120).map((r, i) => {
    const pct = max > 0 ? Math.round((r.score / max) * 100) : 0;
    const [tier, color] = tierOf(pct);
    return h("div", { class: "row" },
      h("span", { class: "pos" }, `${i + 1}.`),
      h("span", { class: "tier", style: `--c:${color}` }, tier),
      sprite(r.hunt.id) ?? h("span", { class: "sprite" }),
      h("div", { class: "row-main" },
        h("div", {}, huntLabel(r.hunt), " ", ...r.hunt.types.map(typeChip)),
        h("div", { class: "row-sub" }, !poke && h("b", {}, `${r.species} `), moveLabel(r.sug)),
        h("div", { class: "row-sub muted" },
          `${fmt(r.sug.killsH)} kills/h · ${fmtCompact(r.sug.xpH)} XP/h · ${fmtCompact(r.sug.goldH)} gold/h · ~${fmt2(r.sug.golpes)} golpes por kill`),
      ),
      h("span", { class: "score", style: `color:${color}` }, String(pct)),
    );
  }));
}

huntLevel.addEventListener("input", debounce(() => renderHunts(), 300));
huntTm.addEventListener("change", () => renderHunts());
[hpSpecies, hpLevel, hpQuality, hpIv, hpAccount].forEach((i) => i.addEventListener("input", debounce(() => renderHunts(), 300)));

// ================= Início =================

showTab(activeTab);
void loadCatalog(false);
