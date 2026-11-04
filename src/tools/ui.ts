// Helpers de DOM e formatação da janela Ferramentas. Nomes e textos vêm do catálogo do jogo:
// tudo entra por textContent, nunca como HTML.

type Child = Node | string | number | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number | boolean | undefined> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === false) continue;
    if (k === "class") el.className = String(v);
    else if (k === "style") el.setAttribute("style", String(v));
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : String(c));
  }
  return el;
}

export const STAT_LABELS = ["HP", "Atk", "Def", "Sp. Atk", "Sp. Def", "Speed"];

export const TYPES = [
  "NORMAL", "FIRE", "WATER", "ELECTRIC", "GRASS", "ICE", "FIGHTING", "POISON", "GROUND",
  "FLYING", "PSYCHIC", "BUG", "ROCK", "GHOST", "DRAGON", "DARK", "STEEL", "FAIRY",
] as const;

const TYPE_PT: Record<string, string> = {
  BUG: "Inseto", DARK: "Sombrio", DRAGON: "Dragão", ELECTRIC: "Elétrico", FAIRY: "Fada", FIGHTING: "Lutador",
  FIRE: "Fogo", FLYING: "Voador", GHOST: "Fantasma", GRASS: "Planta", GROUND: "Terra", ICE: "Gelo",
  NORMAL: "Normal", POISON: "Veneno", PSYCHIC: "Psíquico", ROCK: "Pedra", STEEL: "Aço", WATER: "Água",
};

const TYPE_COLOR: Record<string, string> = {
  BUG: "#a8b820", DARK: "#8a6f5c", DRAGON: "#7a5cf5", ELECTRIC: "#f2c331", FAIRY: "#ee99ac", FIGHTING: "#d04a3a",
  FIRE: "#f08030", FLYING: "#a890f0", GHOST: "#8a6fb8", GRASS: "#62c254", GROUND: "#d8b860", ICE: "#7fd6d6",
  NORMAL: "#a8a878", POISON: "#b45cb4", PSYCHIC: "#f85888", ROCK: "#b8a038", STEEL: "#a8b8c8", WATER: "#5b8ff0",
};

export const typeName = (t: string) => TYPE_PT[t] ?? t;

export function typeChip(t: string): HTMLSpanElement {
  return h("span", { class: "type-chip", style: `--type:${TYPE_COLOR[t] ?? "#8b95a8"}` }, typeName(t));
}

/// Raridade pela qualidade: mesmos limites e nomes do jogo
export function qualityTier(q: number): [string, string] {
  if (q >= 4) return ["Divina", "#dbefff"];
  if (q >= 3) return ["Anciã", "#d4a72c"];
  if (q >= 2) return ["Mítica", "#a77bea"];
  if (q >= 1.7) return ["Lendária", "#ff8c3c"];
  if (q >= 1.5) return ["Épica", "#f0c040"];
  if (q >= 1.3) return ["Rara", "#b06cff"];
  if (q >= 1.1) return ["Incomum", "#7fd4ff"];
  if (q >= 1) return ["Comum", "#63d873"];
  return ["Fraca", "#9aa6b3"];
}

/// Cor da nota de um IV (1 a 32)
export function ivColor(iv: number): string {
  if (iv >= 28) return "var(--iv-gold)";
  if (iv >= 22) return "var(--iv-green)";
  if (iv >= 15) return "var(--iv-blue)";
  return "var(--iv-gray)";
}

export const TIERS: [number, string, string][] = [
  [90, "S", "#f2c665"], [70, "A", "#3fb950"], [50, "B", "#79c0ff"], [30, "C", "#f0883e"], [0, "D", "#8b949e"],
];

export function tierOf(pct: number): [string, string] {
  const t = TIERS.find(([min]) => pct >= min) ?? TIERS[TIERS.length - 1];
  return [t[1], t[2]];
}

const nf0 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 0 });
const nf2 = new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 2 });
const compact = new Intl.NumberFormat("pt-BR", { notation: "compact", maximumFractionDigits: 1 });

export const fmt = (n: number) => nf0.format(n);
export const fmt2 = (n: number) => nf2.format(n);
export const fmtCompact = (n: number) => compact.format(n);
export const fmtMult = (n: number) => "×" + nf2.format(Math.round(n * 100) / 100);

export function sprite(id: number): HTMLImageElement | null {
  // Sprites da PokeAPI só existem para a numeração nacional; formas do jogo (ids altos) ficam sem imagem
  if (!(id >= 1 && id <= 1025)) return null;
  const img = h("img", {
    class: "sprite",
    src: `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${id}.png`,
    alt: "",
    loading: "lazy",
    width: 48,
    height: 48,
  });
  img.addEventListener("error", () => img.remove());
  return img;
}

export function areaTag(area: string): HTMLSpanElement | null {
  const label = { orre: "ORRE", outland: "OUT", nightmare: "NIGHTMARE" }[area];
  return label ? h("span", { class: `area-tag area-${area}` }, label) : null;
}

/// Lê um campo numérico; vazio ou inválido vira null
export function num(input: HTMLInputElement): number | null {
  const raw = input.value.trim().replace(",", ".");
  if (!raw) return null;
  const v = Number(raw);
  return Number.isFinite(v) ? v : null;
}

export function debounce<A extends unknown[]>(fn: (...a: A) => void, ms: number) {
  let t: number | undefined;
  return (...a: A) => {
    window.clearTimeout(t);
    t = window.setTimeout(() => fn(...a), ms);
  };
}

export function store<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // armazenamento indisponível: a preferência só não fica salva
  }
}
