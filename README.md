# PokeGrid Ultra-Light (Tauri v2 + WebView2)

Cliente desktop ultra-otimizado para o jogo idle **Poke Idle World** (`https://poke.idleworld.online`) com suporte integrado para **Twitch Drops** em segundo plano e consumo mínimo de RAM e GPU.

---

## 🚀 Principais Características

- **Grade 2x2 Nativa:** 4 contas simultâneas dividindo a tela perfeitamente.
- **Isolamento de Perfis (`data_directory`):** Sessões, cookies, tokens e `localStorage` 100% independentes entre as 4 contas. Cada pasta de dados roda em seu próprio processo do WebView2.
- **Bônus de live (5 lives em Modo Ghost):** Monitora a lista de canais oficiais (botão **Canais**, em ordem de prioridade) e entra sozinho nos 5 primeiros que estiverem ao vivo, com a página do canal (vídeo 160p mudo + chat logado, que é o que o jogo verifica). Hosts de anúncio são bloqueados (vazavam memória até o renderer morrer).
- **Discord Web:** Abre no drawer lateral para entrar em call; recolhido, fica em 1x1 e a call continua. Usa o mesmo processo de navegador das lives.
- **RAM total na barra:** Soma do app e de todos os processos do WebView2, na mesma métrica da coluna "Memória" do Gerenciador de Tarefas.
- **Modo Foco:** Seletor **Grade · 1 · 2 · 3 · 4** na barra amplia uma conta para jogar em tamanho normal; as outras saem da tela, mas seguem rodando como na grade.
- **Modo Eco nativo + painel de farm:** Oculta as 4 webviews, pede ao WebView2 para liberar memória (`MemoryUsageTargetLevel = Low`) e mostra no lugar um painel com o dia (gold, XP, kills, capturas, shinies, metas), uma linha por conta (pokémon líder, hunt, tempo, gold/h, XP/h, kills/h, melhor e último catch, status como "Parada há X min" ou "Pokébolas p/ ~X"), últimas capturas com filtros, aparições de shiny, drops do dia e exportação em CSV. Clicar numa conta sai do Eco e amplia ela.
- **Sem scripts no jogo:** As webviews do jogo exibem a interface oficial sem injeção de código, auto-login ou automação — as [regras](https://poke.idleworld.online/rules) proíbem. O painel do Modo Eco só **lê** uma cópia das mensagens que o servidor já manda pelo WebSocket de cada conta (protocolo DevTools do WebView2, do lado nativo); nada é injetado nem enviado ao jogo.
- **PokeGrid Remoto (VPS):** As 4 contas rodam 24/7 num Chromium headless na VPS (Docker). Pelo seu domínio, com login e HTTPS, você vê a tela de cada uma, clica e digita nelas (inclusive pelo celular) e tem o mesmo painel do Modo Eco. Um vigia de memória recarrega a conta que passar do teto, o que mantém as 4 em ~4 GB. Guia em [`server/README.md`](server/README.md).
- **Criptografia Nativa Windows (DPAPI):** Credenciais salvas com segurança local do sistema operacional (apenas armazenamento; o login é feito manualmente e fica salvo no perfil).
- **Ferramentas (janela separada):** **Qualidade/IV** (IV de cada atributo pelos números que o jogo mostra, raridade, poder, projeção de nível, efetividade de tipos e comparação de até 3 pokémons), **Tierlist** por elemento (XP/h ou Gold/h, com/sem TM, até o seu nível) e **Onde caçar** (por elemento do dia ou para um pokémon seu). Usa só os dados públicos do site do jogo; não toca nas contas. Modelo e leitura de IV portados do [PokeGrid original](https://github.com/soufoka/PokeGrid-source) (MIT).
- **Motor Matemático em Rust:** IVs, catálogo e modelo de hunts com testes (`cargo test`).

### Consumo medido (contas deslogadas, 5 lives com chat, Discord na tela de login)

| Métrica | Valor |
| :--- | ---: |
| "Memória" do Gerenciador de Tarefas (o que a barra mostra) | ~2,5 GB |
| Memória privada comprometida (commit) | ~3,6 GB |
| Cada conta (privada) | ~290 MB (~210 MB no Modo Eco) |
| 5 lives com chat + Discord (privada) | ~2,3 GB |

---

## 🛠️ Requisitos e Execução

Para compilar e rodar:
1. Ter o **Node.js** (v18+) e **Rust** (`cargo`) instalados no sistema.
2. Instalar dependências leves:
   ```bash
   npm install
   ```
3. Executar em modo desenvolvimento:
   ```bash
   npm run tauri dev
   ```
4. Para gerar o instalador executável:
   ```bash
   npm run tauri build
   ```
