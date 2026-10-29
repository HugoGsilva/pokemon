# PokeGrid Ultra-Light (Tauri v2 + WebView2)

Cliente desktop ultra-otimizado para o jogo idle **Poke Idle World** (`https://poke.idleworld.online`) com suporte integrado para **Twitch Drops** em segundo plano e consumo mínimo de RAM e GPU.

---

## 🚀 Principais Características

- **Grade 2x2 Nativa:** 4 contas simultâneas dividindo a tela perfeitamente.
- **Isolamento de Perfis (`data_directory`):** Sessões, cookies, tokens e `localStorage` 100% independentes entre as 4 contas. Cada pasta de dados roda em seu próprio processo do WebView2.
- **Twitch Drops em Modo Ghost (1px x 1px):** Entra sozinho numa live da categoria, decodifica em 160p silenciado, troca de live se a atual cair e visita `/drops/inventory` a cada 15 min para resgatar drops. Hosts de anúncio são bloqueados (vazavam memória até o renderer morrer).
- **Modo Eco nativo:** Oculta as 4 webviews e pede ao WebView2 para liberar memória (`MemoryUsageTargetLevel = Low`). Nenhum script roda na página do jogo.
- **Sem scripts no jogo:** As webviews do jogo exibem a interface oficial sem injeção de código, auto-login ou automação — as [regras](https://poke.idleworld.online/rules) proíbem.
- **Criptografia Nativa Windows (DPAPI):** Credenciais salvas com segurança local do sistema operacional (apenas armazenamento; o login é feito manualmente e fica salvo no perfil).
- **Motor Matemático de IVs em Rust:** Cálculo combinatório de atributos de alta performance.

### Consumo medido (RAM privada, contas deslogadas, Twitch em 160p)

| | Normal | Modo Eco |
| :--- | ---: | ---: |
| Cada conta | ~290 MB | ~210 MB |
| Barra de controle | ~160 MB | ~165 MB |
| Twitch | ~340–530 MB | ~520 MB |
| **Total** | **~1,8 GB** | **~1,5 GB** |

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
