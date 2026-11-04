import { invoke } from "@tauri-apps/api/core";
import { initFarmPanel } from "./farm";

interface Account {
  id: number;
  user: string;
  pass: string;
}

type Drawer = "none" | "twitch" | "discord";

let isEcoActive = false;
let drawer: Drawer = "none";

// DOM Elements
const btnEco = document.getElementById("btn-eco") as HTMLButtonElement;
const btnTwitch = document.getElementById("btn-twitch") as HTMLButtonElement;
const btnDiscord = document.getElementById("btn-discord") as HTMLButtonElement;
const twitchStatusBadge = document.getElementById("twitch-status-badge") as HTMLSpanElement;
const btnReloadAll = document.getElementById("btn-reload-all") as HTMLButtonElement;
const btnConfig = document.getElementById("btn-config") as HTMLButtonElement;
const accountsModal = document.getElementById("accounts-modal") as HTMLDivElement;
const btnCloseModal = document.getElementById("btn-close-modal") as HTMLButtonElement;
const accountsList = document.getElementById("accounts-list") as HTMLDivElement;
const btnSaveAccounts = document.getElementById("btn-save-accounts") as HTMLButtonElement;

// Nome de cada conta no painel do Modo Eco: o usuário salvo no modal Contas (vazio = "Conta N")
let accountNames: string[] = [];

async function loadAccountNames() {
  try {
    const accounts = await invoke<Account[]>("get_accounts");
    accountNames = [1, 2, 3, 4].map((i) => accounts.find((a) => a.id === i)?.user ?? "");
  } catch (err) {
    console.warn("Could not load account names:", err);
  }
}

// Painel do Modo Eco: aparece no lugar da grade enquanto as contas estão ocultas
const farmPanel = initFarmPanel({
  names: () => accountNames,
  openAccount: (slot) => void openAccount(slot),
});

function renderEcoButton() {
  btnEco.querySelector(".status-dot")?.classList.toggle("green", !isEcoActive);
  btnEco.classList.toggle("active", isEcoActive);
  if (isEcoActive) farmPanel.show();
  else farmPanel.hide();
}

function renderDrawerButtons() {
  twitchStatusBadge.textContent = drawer === "twitch" ? "5 lives · 1 visível" : "5 lives · Ghost";
  btnTwitch.classList.toggle("active", drawer === "twitch");
  btnDiscord.classList.toggle("active", drawer === "discord");
  // O painel do Modo Eco abre espaço para o drawer (as webviews nativas ficam por cima do HTML)
  document.body.classList.toggle("drawer-open", drawer !== "none");
}

// Twitch e Discord dividem o drawer lateral: abrir um recolhe o outro (que segue rodando em Ghost)
async function toggleDrawer(target: Exclude<Drawer, "none">) {
  const previous = drawer;
  drawer = drawer === target ? "none" : target;
  renderDrawerButtons();
  try {
    await invoke("set_drawer", { target: drawer });
  } catch (err) {
    console.warn("Tauri invoke set_drawer error:", err);
    drawer = previous;
    renderDrawerButtons();
  }
}

async function setEco(enabled: boolean) {
  if (enabled === isEcoActive) return;
  isEcoActive = enabled;
  renderEcoButton();
  try {
    await invoke("toggle_eco_mode", { enabled });
  } catch (err) {
    console.warn("Tauri invoke toggle_eco_mode error:", err);
    isEcoActive = !enabled;
    renderEcoButton();
  }
}

// Eco Mode Toggle
btnEco?.addEventListener("click", () => void setEco(!isEcoActive));

// Modo Foco: 0 = grade; 1..4 amplia uma conta (as outras saem da tela, mas seguem rodando)
let focus = 0;
const viewButtons = document.querySelectorAll<HTMLButtonElement>("#view-switch [data-focus]");

function renderViewSwitch() {
  viewButtons.forEach((b) => b.classList.toggle("active", Number(b.dataset.focus) === focus));
}

async function setFocus(account: number) {
  const previous = focus;
  // Clicar na conta já ampliada volta para a grade
  focus = focus === account ? 0 : account;
  renderViewSwitch();
  try {
    await invoke("set_focus", { account: focus });
  } catch (err) {
    console.warn("Tauri invoke set_focus error:", err);
    focus = previous;
    renderViewSwitch();
  }
}

viewButtons.forEach((b) => b.addEventListener("click", () => setFocus(Number(b.dataset.focus))));

// Linha do painel do Modo Eco: mostra as contas de novo e amplia a escolhida
async function openAccount(slot: number) {
  await setEco(false);
  if (focus !== slot) await setFocus(slot);
}

btnTwitch?.addEventListener("click", () => toggleDrawer("twitch"));
btnDiscord?.addEventListener("click", () => toggleDrawer("discord"));

// RAM total do app (todos os processos juntos), atualizada a cada 5s
const ramUsage = document.getElementById("ram-usage") as HTMLSpanElement;

async function updateRamUsage() {
  try {
    const bytes = await invoke<number>("get_memory_usage");
    const mb = bytes / 1024 / 1024;
    ramUsage.textContent = mb >= 1024 ? `RAM ${(mb / 1024).toFixed(2)} GB` : `RAM ${Math.round(mb)} MB`;
  } catch (err) {
    console.warn("Tauri invoke get_memory_usage error:", err);
  }
}

void updateRamUsage();
setInterval(updateRamUsage, 5000);

// Janela Ferramentas (Qualidade/IV, Tierlist, Onde caçar): fora da grade, as contas não são tocadas
document.getElementById("btn-tools")?.addEventListener("click", async () => {
  try {
    await invoke("open_tools");
  } catch (err) {
    console.warn("Tauri invoke open_tools error:", err);
  }
});

// Reload All
btnReloadAll?.addEventListener("click", async () => {
  try {
    await invoke("reload_all_accounts");
  } catch (err) {
    console.warn("Tauri invoke reload_all_accounts error:", err);
  }
});

// Modal Management
// As webviews das contas são nativas e ficam por cima deste HTML: precisam ser escondidas enquanto um modal está aberto
async function setOverlay(open: boolean) {
  try {
    await invoke("set_overlay_open", { open });
  } catch (err) {
    console.warn("Tauri invoke set_overlay_open error:", err);
  }
}

async function openModal() {
  await setOverlay(true);
  accountsModal.classList.remove("hidden");
  await renderAccounts();
}

async function closeModal() {
  accountsModal.classList.add("hidden");
  accountsList.replaceChildren();
  await setOverlay(false);
}

btnConfig?.addEventListener("click", openModal);
btnCloseModal?.addEventListener("click", closeModal);

// Canais oficiais do bônus de live (um por linha, em ordem de prioridade)
const btnChannels = document.getElementById("btn-channels") as HTMLButtonElement;
const channelsModal = document.getElementById("channels-modal") as HTMLDivElement;
const channelsInput = document.getElementById("channels-input") as HTMLTextAreaElement;
const btnCloseChannels = document.getElementById("btn-close-channels") as HTMLButtonElement;
const btnSaveChannels = document.getElementById("btn-save-channels") as HTMLButtonElement;

async function openChannelsModal() {
  await setOverlay(true);
  try {
    const channels = await invoke<string[]>("get_twitch_channels");
    channelsInput.value = channels.join("\n");
  } catch (err) {
    console.warn("Tauri invoke get_twitch_channels error:", err);
  }
  channelsModal.classList.remove("hidden");
  channelsInput.focus();
}

async function closeChannelsModal() {
  channelsModal.classList.add("hidden");
  await setOverlay(false);
}

btnChannels?.addEventListener("click", openChannelsModal);
btnCloseChannels?.addEventListener("click", closeChannelsModal);

btnSaveChannels?.addEventListener("click", async () => {
  const channels = channelsInput.value.split(/[\n,;]+/);
  try {
    // O Rust devolve a lista normalizada (minúsculas, sem URL/@, sem repetidos)
    channelsInput.value = (await invoke<string[]>("save_twitch_channels", { channels })).join("\n");
    await closeChannelsModal();
  } catch (err) {
    alert("Erro ao salvar a lista de canais: " + err);
  }
});

document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (!accountsModal.classList.contains("hidden")) void closeModal();
  if (!channelsModal.classList.contains("hidden")) void closeChannelsModal();
});

function createInput(type: string, id: string, placeholder: string, value: string): HTMLInputElement {
  const input = document.createElement("input");
  input.type = type;
  input.id = id;
  input.placeholder = placeholder;
  // Atribuição direta: valores com aspas ou "<" não quebram o markup
  input.value = value;
  return input;
}

async function renderAccounts() {
  accountsList.replaceChildren();
  let accounts: Account[] = [];
  try {
    accounts = await invoke<Account[]>("get_accounts");
  } catch (err) {
    console.warn("Could not load accounts:", err);
  }

  for (let i = 1; i <= 4; i++) {
    const acc = accounts.find((a) => a.id === i) || { id: i, user: "", pass: "" };
    const row = document.createElement("div");
    row.className = "account-row";

    const label = document.createElement("span");
    label.textContent = `Conta ${i}`;

    row.append(
      label,
      createInput("text", `acc-user-${i}`, "Usuário", acc.user),
      createInput("password", `acc-pass-${i}`, "Senha", acc.pass),
    );
    accountsList.appendChild(row);
  }
}

btnSaveAccounts?.addEventListener("click", async () => {
  const accounts: Account[] = [];
  for (let i = 1; i <= 4; i++) {
    const userInput = document.getElementById(`acc-user-${i}`) as HTMLInputElement | null;
    const passInput = document.getElementById(`acc-pass-${i}`) as HTMLInputElement | null;
    accounts.push({
      id: i,
      user: userInput?.value.trim() || "",
      pass: passInput?.value || "",
    });
  }

  try {
    await invoke("save_accounts", { accounts });
    accountNames = accounts.map((a) => a.user);
    farmPanel.namesChanged();
    await closeModal();
  } catch (err) {
    alert("Erro ao salvar credenciais com DPAPI: " + err);
  }
});

void loadAccountNames().then(() => farmPanel.namesChanged());
