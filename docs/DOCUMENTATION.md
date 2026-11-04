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

- **Host Nativo (Rust / Tauri v2):** Orquestrador principal responsável pelo ciclo de vida das janelas, injeção de scripts (somente na Twitch), criptografia segura via DPAPI, motor matemático de IVs e o modelo de hunts da janela Ferramentas.
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
5. **Painel de farm** (`src/farm.ts`): aparece no lugar da grade enquanto o Modo Eco está ligado e atualiza a cada 3 s (só enquanto visível).

#### 3.2.1 Painel do Modo Eco: de onde vêm os números
- **Leitura passiva** (`src-tauri/src/farm.rs`): cada webview de conta liga o domínio `Network` do protocolo DevTools do WebView2 (`CallDevToolsProtocolMethod("Network.enable")` com buffers 0, então nenhum corpo de resposta fica guardado) e assina `Network.webSocketCreated`, `webSocketFrameReceived` e `webSocketClosed`. O app recebe, no lado nativo, uma cópia dos frames que o servidor já manda para a página. Nada roda na página, nada é enviado ao jogo e a página não tem como perceber. Só o socket `wss://poke.idleworld.online/ws…` conta. O domínio é religado a cada carga de página.
- **Contabilidade** (`src-tauri/src/domain/farm.rs`, com testes): as mesmas mensagens que o PokeGrid original lê (MIT). `field-init`/`field-none` (entrou/saiu da hunt), `field-kill` (XP, loot, shiny), `catch-result` (pokébola gasta, sucesso), `pokes`/`pokes-chunk`/`poke-delta` (a caixa: id novo depois da primeira lista é captura, com IV e qualidade), `poke-xp` (nível do líder), `balls` (estoque e preço das pokébolas) e `field` (time derrotado).
- **Gold** é estimado: loot a preço de NPC (do `items.json` público, o mesmo preço do Hunt Analyzer do jogo) + valor de venda das capturas − preço das pokébolas gastas. O Hunt Analyzer do próprio jogo não é consultado, porque pedir os dados dele seria mandar mensagem pelo socket.
- **Sessão** = a hunt atual de cada conta (troca de hunt ou **Zerar** começa outra; a anterior vira uma linha da exportação **Hunts**). As taxas por hora usam o tempo na hunt; fora dela, até o último abate.
- **Status:** *Aguardando…* (sem mensagens: login ou carregando), *Desconectada* (socket fechado há mais de 20 s), *Time derrotado*, *⚠ Parada há X min* (10 min sem abate na hunt), *⚠ Pokébolas p/ ~X* (estoque ÷ ritmo da sessão, quando acaba em menos de 4 h), *Caçando* e *Na cidade*.
- **Histórico** em `farm_history.json` na pasta de dados do app, gravado a cada minuto e ao fechar: somas por hora (90 dias; os drops por item ficam 8 dias), as últimas 500 capturas, 300 shinies e 500 hunts. **⭳ Dia** e **⭳ Hunts** gravam CSV (separador `;`, para o Excel em português) na pasta Downloads. Nomes das contas vêm do modal **Contas**.
- **Limites conhecidos:** filhote chocado no Breeding e pokémon comprado no Mercado entram como captura (o jogo os manda do mesmo jeito); validade de pokébola temporária não é considerada no estoque; "NOVO" quer dizer primeiro da espécie na caixa da conta, não na Pokédex.

### 3.3 Twitch: bônus de live (5 lives em Modo Ghost)
**Como o jogo detecta:** o bot do jogo é moderador dos canais oficiais e lê quem está **conectado ao chat** logado na conta da Twitch vinculada ("logged out, incognito, embedded player or closed chat do not count"). Só contam canais oficiais com o selo ⚡ BONUS ON. O bônus vale para a família inteira (conta o membro com mais lives), então uma conta da Twitch cobre as 4 contas do jogo da mesma família.

- **5 lives** (`TWITCH_LIVES = 5` em `lib.rs`): webviews `twitch1`..`twitch5` numa **única** pasta de dados (`profiles/twitch`) — um processo de navegador/GPU/rede e o mesmo login da Twitch.
- **Página completa do canal** (vídeo + chat), não o player popout: o chat precisa estar carregado e logado para o jogo encontrar o usuário.
- **Lista de canais:** botão **Canais** (um por linha, em ordem de prioridade), salva em `twitch_channels.json` na pasta de dados do app; sem arquivo usa `DEFAULT_CHANNELS` (`webviews/channels.rs`). Aceita nome, `@nome` ou URL.
- **Monitoramento:** a cada 60 s cada live consulta quais canais da lista estão ao vivo (consulta pública `gql.twitch.tv`, a mesma do site). Os 5 primeiros ao vivo são os desejados; cada live **fica no canal atual** enquanto ele segue entre eles (evita sair do chat à toa) e só a que ficou sem canal pega o próximo livre. Sem canal disponível, a live espera em `www.twitch.tv/robots.txt` (página mínima).
- **Sem repetição:** cada live registra seu canal no `localStorage` de `www.twitch.tv` (`__pgSlots`); se duas caírem no mesmo canal, a de número maior cede.
- **Drawer:** o botão **Twitch** mostra a live 1; com o drawer aberto a navegação é manual (o monitoramento pausa nela).
- **Ghost:** vídeo em 160p30 (quando o canal oferece), mudo, 1px e `opacity: 0.01`; menu/barra lateral ocultos; **o chat não é ocultado nem bloqueado** (bloquear o WebSocket do chat fazia a Twitch reconectar em loop até o renderer morrer com "Out of Memory").
- **Anúncios bloqueados por DNS:** `--host-resolver-rules` (só no perfil da Twitch/Discord) resolve `*.amazon-adsystem.com`, `*.advertising.amazon.dev`, `edge.ads.twitch.tv` e `*.scorecardresearch.com` para NOTFOUND — os iframes de anúncio vazavam memória (2,8 GB em ~10 min).
- **Medido:** ~2,5 GB no total do app (métrica "Memória" do Gerenciador de Tarefas) com 4 contas, 5 lives com chat e o Discord.

### 3.4 Discord Web
- Webview `discord` (`https://discord.com/app`) sem script injetado, na mesma pasta de dados das lives (sem processo de navegador extra).
- O botão **Discord** mostra no drawer lateral; Twitch e Discord dividem o drawer (abrir um recolhe o outro). Recolhido, fica em 1x1 e a call continua.
- Microfone: o WebView2 mostra o pedido de permissão padrão na primeira call — aceite com o drawer aberto.

### 3.5 RAM total na barra
- `metrics.rs` percorre a árvore de processos a partir do PID do app (Toolhelp32) e soma o conjunto de trabalho privado (`PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`) — a mesma métrica da coluna "Memória" do Gerenciador de Tarefas, que lista os processos do WebView2 separados.
- A barra atualiza a cada 5 s.

### 3.6 Segurança de Credenciais com DPAPI
- Credenciais salvas através do modal **Contas** são criptografadas diretamente com a API do Windows `CryptProtectData` (`CRYPT_INTEGER_BLOB`).
- O arquivo gerado (`accounts.enc`) só pode ser descriptografado pelo mesmo usuário no mesmo computador.
- Gravação atômica via arquivo temporário (`accounts.enc.tmp`) prevenindo corrupção em caso de desligamento abrupto.

- **Não há auto-login:** preencher o login por script é automação proibida pelas regras do jogo. Faça login uma vez em cada quadrante; a sessão fica salva nos cookies do perfil (`profiles/contaN`).

### 3.7 Conformidade com as Regras do Jogo
Regras em [poke.idleworld.online/rules](https://poke.idleworld.online/rules):
- **Até 4 contas** por jogador (mais que isso leva a exclusão permanente) — o app cria exatamente `MAX_ACCOUNTS = 4` quadrantes.
- **Scripts, extensões, macros e automações sem permissão da staff são proibidos** — as webviews do jogo não recebem `initialization_script`, `eval` nem cliques automáticos. "Recarregar Tudo" usa o reload nativo (equivale ao F5).
- Pontos de atenção que dependem de interpretação da staff: as flags que impedem o Chromium de desacelerar abas em segundo plano, o uso simultâneo de 4 contas ("uso coordenado" pode ser visto como irregularidade) e a leitura passiva do WebSocket pelo painel do Modo Eco (não automatiza nada, mas é uma ferramenta lendo dados do jogo). Na dúvida, peça autorização no Discord do jogo.
- A automação da Twitch (troca de live, resgate de drops, bloqueio de anúncios) não afeta o jogo, mas está sujeita aos termos de uso da Twitch.

### 3.8 Motor Matemático de IVs Nativo
Implementado em Rust compilado (`src-tauri/src/domain/iv_math.rs`), executa a fórmula do jogo:
$$\text{stat} = \text{round}\left( (\text{base} + 2 \times \text{growth}) \times \frac{\text{level}}{100} \times \text{quality}^{\text{exp}} \right)$$
- Expoentes específicos por atributo:
  - HP: `0.95`, Velocidade: `0.95`, Ataque/Defesa/SpA/SpD: `0.80`.
- Varredura de combinações instantânea sem travamentos na interface.
- **Leitura de IV pelos atributos** (`normalize_ivs`): a qualidade que o jogo mostra vem com 2 casas, então a de verdade é procurada numa janela de meia casa (apertada pelo Poder, quando informado), primeiro nos valores de 3 casas que o jogo sorteia. O IV total, quando informado, elimina combinações que não somam. Em nível baixo dois IVs podem dar o mesmo atributo: aí sai a faixa (12-13). Portado do `iv-math.js` do [PokeGrid original](https://github.com/soufoka/PokeGrid-source) (MIT), com os mesmos casos de teste.

### 3.9 Janela Ferramentas (Qualidade/IV, Tierlist, Onde caçar)
Botão **Ferramentas** na barra. Abre uma janela separada (`tools.html`, declarada com `create: false` no `tauri.conf.json`): **nenhuma conta é ocultada, movida ou tocada**, e fechar a janela libera a memória dela. Ela usa a mesma pasta de dados da barra, então não cria outra árvore de processos do WebView2.

- **Dados:** o Rust (`src-tauri/src/tools.rs`) baixa os arquivos **públicos** do site do jogo (`/game/creatures.json`, `/game/items.json`, `/api/game/map-markers`), monta o catálogo (`domain/catalog.rs`) e guarda em `gamedata.json` na pasta de dados do app. Rebaixa depois de 24 h ou no botão **Atualizar dados**; sem internet usa o cache e só tenta de novo depois de 10 min. Nada é lido das páginas das contas.
- **Qualidade / IV:** espécie, nível, qualidade, os 6 atributos e, se quiser, IV total e Poder. Mostra o IV de cada atributo (cor: dourado ≥28, verde ≥22, azul ≥15), a raridade pela qualidade (Comum … Divina), IV total, Poder, melhor e pior atributo, quanto cada um subiria com IV 32 e a projeção em outro nível. Também mostra a efetividade de tipos com a regra da hunt e os golpes por nível. **📌 Fixar** guarda até 3 pokémons para a tabela **Comparar**; **Onde caçar com ele →** leva o pokémon para a aba de hunts.
- **Tierlist:** todas as espécies por elemento (ou **Geral**, que soma o rendimento em todas as hunts), nota de 0 a 100, por **XP/h** ou **Gold/h** (loot esperado a preço de NPC) e **com TM** opcional. Só entram hunts e espécies que o seu nível alcança.
- **Onde caçar:** **Por elemento** (o Tipo do Dia do jogo: escolha o elemento e o nível; sai a hunt e o melhor pokémon para ela) ou **Meu pokémon** (espécie, nível, qualidade, IV total e nível da conta; saem as melhores hunts para ele).
- **Modelo (`domain/hunt_model.rs`):** o do PokeGrid original. O melhor golpe aprendido conta com efetividade amplificada (×2→×2,5, ×4→×5,5, resistências ÷1,5), STAB ×1,5, físico contra Defesa e especial contra Defesa Especial do selvagem (qualidade 1, IV 96 no nível da hunt), e a chance de matar de um golpe (dano real log-normal, σ 0,5). Tempo por kill = 2,1 s por golpe + 3,1 s. TM elemental = golpe extra de poder 300 a cada 10 s em 2 alvos; AoE = golpe normal em 2 alvos. **Diferença do original:** lá o app se calibra medindo o farm dentro do jogo (dano real, kills/h); aqui isso exigiria injetar script nas contas, o que as regras proíbem. Então os números são estimativas para comparar opções, não previsões exatas.

### 3.10 PokeGrid Remoto (VPS)
Servidor em Rust (`server/`) para rodar as contas numa VPS e acompanhar pelo navegador (`remote.html`). O guia de deploy, as variáveis e a segurança estão em [`server/README.md`](../server/README.md).

- **Um Chromium headless por conta** (`chromium-headless-shell` do Debian, `--disable-gpu`), cada um com o perfil em `/data/contaN`. Processos separados custam cerca de 0,1 GB a mais que abas num navegador só, mas dão a RAM exata de cada conta, e uma conta travada não derruba as outras.
- **Protocolo DevTools, sem script na página:**
  - a tela vem do `Page.startScreencast` (JPEG só quando a tela muda, e só enquanto alguém olha; o ack atrasado limita os quadros por segundo);
  - os cliques e o teclado de quem está na página vão por `Input.dispatch*`;
  - o painel lê o WebSocket do jogo pelo domínio `Network`, como no app.
- **Mesma contabilidade do app:** `server/src/main.rs` inclui `src-tauri/src/domain/farm.rs` e `catalog.rs` por `#[path]`. O CSV (`export_csv`) também é compartilhado. O painel (`src/farm.ts` + `src/farm-panel.html`) recebe uma `FarmApi`: comandos do Tauri no app, HTTP na página remota.
- **Vigia de memória** (`server/src/account.rs`):
  - a cada 30 s lê a PSS da árvore de processos de cada Chromium (`/proc`);
  - recarrega a conta que passar de `PG_MEM_ACCOUNT_MB`, ou a maior quando a soma passa de `PG_MEM_TOTAL_MB`;
  - numa hunt, espera o próximo `field-kill` (até 90 s);
  - se a conta recarregou há menos de 10 min e passou do teto de novo, reinicia o Chromium dela.
  - Motivo: no PC, a página de uma conta cresce de cerca de 0,3 GB para 1–2 GB em 20 h, e o F5 zera isso.
- **Página:**
  - grade ou conta ampliada (vídeo maior e com mais quadros por segundo);
  - toque vira clique, e arrastar vira rolagem;
  - teclado do celular pelo botão ⌨;
  - painel do Modo Eco (com ele aberto, nada transmite);
  - selo de RAM por conta;
  - login por Basic auth, com bloqueio depois de 10 senhas erradas num minuto e conferência da origem nos pedidos que mudam algo.

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
| **Ampliar uma conta (Modo Foco)** | Use o seletor **Grade · 1 · 2 · 3 · 4** na barra: o número amplia aquela conta na área toda (descontando o drawer, se aberto); clique de novo nele ou em **Grade** para voltar ao 2x2. As outras contas saem da tela sem serem ocultadas e seguem rodando como na grade. |
| **Alternar Modo Eco** | Clique no botão **Modo Eco** para ocultar as contas (seguem abertas, com menos RAM) e ver o painel de farm, ou para exibi-las de novo. No painel, clique numa linha da tabela para sair do Eco já com aquela conta ampliada. |
| **Visualizar / Ocultar Twitch** | Clique em **Twitch** para mostrar a live 1 no drawer lateral ou recolhê-la para o Ghost Mode (as 5 lives seguem rodando). |
| **Discord** | Clique em **Discord** para mostrar/recolher o Discord Web; a call continua com ele recolhido. |
| **Recarregar Contas** | O botão **Recarregar Tudo** recarrega os 4 quadrantes caso haja perda de conexão. |
| **Ferramentas** | Clique em **Ferramentas** para abrir a janela de Qualidade/IV, Tierlist e Onde caçar. Ela é separada da grade: as contas continuam como estão. |
| **Configurar Contas** | Clique em **Contas** para guardar usuário/senha criptografados (só armazenamento; o login no jogo é manual). |
