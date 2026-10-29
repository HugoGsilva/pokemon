# PokeGrid Ultra-Light — Documentação Técnica e Operacional

> **Versão:** 1.0.0  
> **Stack:** Tauri v2 (Rust) + WebView2 (Microsoft Edge) + TypeScript / Vite

---

## 1. Visão Geral

O **PokeGrid Ultra-Light** é um cliente desktop de alta performance projetado para executar simultaneamente até **4 contas do jogo idle [Poke Idle World](https://poke.idleworld.online)** organizadas em grade 2x2, integrado a uma **instância de Twitch Drops em segundo plano**, priorizando o menor consumo possível de memória RAM e GPU.

### Comparativo com a versão Electron tradicional:
- **Consumo de RAM:** Menor que a versão Electron, mas cada perfil isolado abre seu próprio processo do WebView2 (5 no total) — meça no Gerenciador de Tarefas antes de divulgar números.
- **GPU em Background:** Próximo de **0%** com o Modo Eco (webviews ocultas não renderizam) e o Ghost Mode da Twitch.
- **Segurança:** Criptografia local nativa do Windows (DPAPI) sem bibliotecas externas pesadas.

---

## 2. Arquitetura do Sistema

- **Host Nativo (Rust / Tauri v2):** Orquestrador principal responsável pelo ciclo de vida das janelas, injeção de scripts (somente na Twitch), criptografia segura via DPAPI e motor matemático de IVs.
- **Interface de Controle (HUD):** Barra superior de 44px em HTML5/TypeScript permitindo alternar Modo Eco, abrir/fechar o drawer da Twitch, recarregar contas e configurar credenciais.
- **Camada WebView2 (Microsoft Edge):** 4 webviews em grade com isolamento estrito de dados (`profiles/conta1` a `profiles/conta4`) + 1 webview dedicada para a Twitch (`profiles/twitch`).

---

## 3. Funcionalidades Detalhadas

### 3.1 Multi-Conta Isolada (Perfis Dedicados)
- Cada conta roda com seu próprio diretório de persistência (`profiles/conta1` a `profiles/conta4`), gerando bancos SQLite separados para cookies, tokens de sessão e `localStorage`.
- As instâncias compartilham os binários do Edge WebView2 instalados no sistema, mas cada pasta de dados tem seu próprio processo de navegador.
- Popups (`window.open`) são permitidos nas webviews do jogo: o "Continuar com o Google" abre a janela de login do Google e recebe o token via `window.opener`. Sem `on_new_window` o wry bloqueia todo popup.
- `--disable-site-isolation-trials` **não** é usado: o captcha do Cloudflare (Turnstile) no login detecta o isolamento desligado e retorna "Falha na verificação".
- As flags de desempenho (`--disable-background-timer-throttling`, `--disable-renderer-backgrounding` etc.) são aplicadas a cada webview filha via `additional_browser_args` — o `additionalBrowserArgs` do `tauri.conf.json` só vale para a barra de controle.

### 3.2 Modo Eco (nativo)
Ao acionar o botão **Modo Eco**:
1. **Ocultação:** As 4 webviews das contas são ocultadas (`Webview::hide`), o equivalente a minimizar o navegador — elas param de renderizar e a barra mostra um aviso no lugar da grade.
2. **Liberação de memória:** Cada webview recebe `MemoryUsageTargetLevel = Low` (`ICoreWebView2_19`), que faz o WebView2 descartar caches. Medido: ~290 MB → ~210 MB por conta.
3. **Timers do jogo:** As flags `--disable-background-timer-throttling`, `--disable-renderer-backgrounding` e `IntensiveWakeUpThrottling` desligado evitam que o Chromium atrase os timers da página oculta.
4. **Nada é injetado no jogo:** o Modo Eco atua só no WebView2. Ainda não foi confirmado com conta logada se o jogo reduz o farm quando a página fica `document.hidden`; se reduzir, use a [Sleep Mode](https://poke.idleworld.online/pokepedia) do próprio jogo.

### 3.3 Twitch Drops em Modo Ghost (1px x 1px)
A Twitch normalmente consome centenas de megabytes devido a decodificação de vídeo e chat:
- **Armadilha evitada:** Ocultar o vídeo com `display: none` congela a decodificação via `IntersectionObserver`, fazendo com que a telemetria (`spade.twitch.tv`) pause a contagem do Drop.
- **Solução implementada:**
  1. Resolução forçada para **160p30** no `localStorage` antes da inicialização do player.
  2. Redimensionamento do container de vídeo para **1px x 1px** com `opacity: 0.01` posicionado no canto da tela. O stream continua ativo para o sistema de Drops, mas consome praticamente 0% de GPU.
  3. Áudio silenciado nativamente (`video.muted = true`).
  4. **Chat oculto, não bloqueado:** bloquear o WebSocket do chat fazia a Twitch reconectar em loop (~100x/min) até o renderer morrer com "Out of Memory"; agora o chat só é escondido via CSS.
  5. **Bloqueio de anúncios por DNS:** `--host-resolver-rules` (apenas na webview da Twitch) resolve `*.amazon-adsystem.com`, `*.advertising.amazon.dev`, `edge.ads.twitch.tv` e `*.scorecardresearch.com` para NOTFOUND. Os iframes de anúncio eram recriados sem serem liberados (2,8 GB em ~10 min); com o bloqueio a Twitch fica estável em ~340–530 MB.
  6. **Seleção de Live:** A webview abre o diretório da categoria e entra na primeira live listada. Se o vídeo ficar parado por ~2 min, volta ao diretório e escolhe outra.
  7. **Auto-Claim:** A cada 15 min visita `/drops/inventory`, clica em "Claim/Resgatar" e retorna à live. Bônus de pontos do canal também são coletados.
  8. Toda a automação só roda em Ghost Mode; com o drawer aberto a navegação é manual.

### 3.4 Segurança de Credenciais com DPAPI
- Credenciais salvas através do modal **Contas** são criptografadas diretamente com a API do Windows `CryptProtectData` (`CRYPT_INTEGER_BLOB`).
- O arquivo gerado (`accounts.enc`) só pode ser descriptografado pelo mesmo usuário no mesmo computador.
- Gravação atômica via arquivo temporário (`accounts.enc.tmp`) prevenindo corrupção em caso de desligamento abrupto.

- **Não há auto-login:** preencher o login por script é automação proibida pelas regras do jogo. Faça login uma vez em cada quadrante; a sessão fica salva nos cookies do perfil (`profiles/contaN`).

### 3.5 Conformidade com as Regras do Jogo
Regras em [poke.idleworld.online/rules](https://poke.idleworld.online/rules):
- **Até 4 contas** por jogador (mais que isso leva a exclusão permanente) — o app cria exatamente `MAX_ACCOUNTS = 4` quadrantes.
- **Scripts, extensões, macros e automações sem permissão da staff são proibidos** — as webviews do jogo não recebem `initialization_script`, `eval` nem cliques automáticos. "Recarregar Tudo" usa o reload nativo (equivale ao F5).
- Pontos de atenção que dependem de interpretação da staff: as flags que impedem o Chromium de desacelerar abas em segundo plano e o uso simultâneo de 4 contas ("uso coordenado" pode ser visto como irregularidade). Na dúvida, peça autorização no Discord do jogo.
- A automação da Twitch (troca de live, resgate de drops, bloqueio de anúncios) não afeta o jogo, mas está sujeita aos termos de uso da Twitch.

### 3.6 Motor Matemático de IVs Nativo
Implementado em Rust compilado (`src-tauri/src/domain/iv_math.rs`), executa a fórmula do jogo:
$$\text{stat} = \text{round}\left( (\text{base} + 2 \times \text{growth}) \times \frac{\text{level}}{100} \times \text{quality}^{\text{exp}} \right)$$
- Expoentes específicos por atributo:
  - HP: `0.95`, Velocidade: `0.95`, Ataque/Defesa/SpA/SpD: `0.80`.
- Varredura de combinações instantânea sem travamentos na interface.

---

## 4. Como Executar e Compilar

### Pré-requisitos:
- **Node.js** (v18+)
- **Rust e Cargo** (versão 1.75+ recomendada)
- **Microsoft Edge WebView2** (já embutido no Windows 10/11)

### Modo de Desenvolvimento:
Para iniciar com recarga automática:
```powershell
npm run tauri dev
```

### Gerar Executável / Instalador Final:
Para gerar o instalador `.msi` e o `.exe` compilados para produção:
```powershell
npm run tauri build
```
O executável final estará disponível em `src-tauri/target/release/`.

---

## 5. Atalhos e Operação

| Ação | Como Utilizar |
| :--- | :--- |
| **Alternar Modo Eco** | Clique no botão **Modo Eco** para ocultar as contas (seguem abertas, com menos RAM) ou exibi-las de novo. |
| **Visualizar / Ocultar Twitch** | Clique em **Twitch Drops** para expandir o player lateral ou recolhê-lo para o Ghost Mode. |
| **Recarregar Contas** | O botão **Recarregar Tudo** recarrega os 4 quadrantes caso haja perda de conexão. |
| **Configurar Contas** | Clique em **Contas** para guardar usuário/senha criptografados (só armazenamento; o login no jogo é manual). |
