import { invoke } from "@tauri-apps/api/core";

interface Account {
  id: number;
  user: string;
  pass: string;
}

let isEcoActive = false;
let isTwitchDrawerOpen = false;

// DOM Elements
const btnEco = document.getElementById("btn-eco") as HTMLButtonElement;
const btnTwitch = document.getElementById("btn-twitch") as HTMLButtonElement;
const twitchStatusBadge = document.getElementById("twitch-status-badge") as HTMLSpanElement;
const btnReloadAll = document.getElementById("btn-reload-all") as HTMLButtonElement;
const btnConfig = document.getElementById("btn-config") as HTMLButtonElement;
const accountsModal = document.getElementById("accounts-modal") as HTMLDivElement;
const btnCloseModal = document.getElementById("btn-close-modal") as HTMLButtonElement;
const accountsList = document.getElementById("accounts-list") as HTMLDivElement;
const btnSaveAccounts = document.getElementById("btn-save-accounts") as HTMLButtonElement;
const ecoPlaceholder = document.getElementById("eco-placeholder") as HTMLElement;

function renderEcoButton() {
  btnEco.querySelector(".status-dot")?.classList.toggle("green", !isEcoActive);
  btnEco.classList.toggle("active", isEcoActive);
  ecoPlaceholder.classList.toggle("hidden", !isEcoActive);
}

function renderTwitchBadge() {
  twitchStatusBadge.textContent = isTwitchDrawerOpen ? "Visível (160p)" : "Ghost (160p)";
}

// Eco Mode Toggle
btnEco?.addEventListener("click", async () => {
  isEcoActive = !isEcoActive;
  renderEcoButton();
  try {
    await invoke("toggle_eco_mode", { enabled: isEcoActive });
  } catch (err) {
    console.warn("Tauri invoke toggle_eco_mode error:", err);
    isEcoActive = !isEcoActive;
    renderEcoButton();
  }
});

// Twitch Drawer Toggle
btnTwitch?.addEventListener("click", async () => {
  isTwitchDrawerOpen = !isTwitchDrawerOpen;
  renderTwitchBadge();
  try {
    await invoke("toggle_twitch_drawer", { open: isTwitchDrawerOpen });
  } catch (err) {
    console.warn("Tauri invoke toggle_twitch_drawer error:", err);
    isTwitchDrawerOpen = !isTwitchDrawerOpen;
    renderTwitchBadge();
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
// As webviews das contas são nativas e ficam por cima deste HTML: precisam ser escondidas enquanto o modal está aberto
async function openModal() {
  try {
    await invoke("set_overlay_open", { open: true });
  } catch (err) {
    console.warn("Tauri invoke set_overlay_open error:", err);
  }
  accountsModal.classList.remove("hidden");
  await renderAccounts();
}

async function closeModal() {
  accountsModal.classList.add("hidden");
  accountsList.replaceChildren();
  try {
    await invoke("set_overlay_open", { open: false });
  } catch (err) {
    console.warn("Tauri invoke set_overlay_open error:", err);
  }
}

btnConfig?.addEventListener("click", openModal);
btnCloseModal?.addEventListener("click", closeModal);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !accountsModal.classList.contains("hidden")) {
    void closeModal();
  }
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
    await closeModal();
  } catch (err) {
    alert("Erro ao salvar credenciais com DPAPI: " + err);
  }
});
