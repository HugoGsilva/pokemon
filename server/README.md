# PokeGrid Remoto (VPS)

As 4 contas rodam 24/7 num Chromium headless na VPS. Pelo seu domínio você vê a tela de cada uma, clica e digita
nelas (mouse, toque e teclado), e tem o painel do Modo Eco. O PC pode ficar desligado.

```
VPS (Docker)
├─ pokegrid-server (Rust)
│   ├─ 1 Chromium headless por conta (perfil próprio em /data/contaN: o login sobrevive a reinícios)
│   ├─ tela:   Page.startScreencast → JPEG só quando a tela muda, só para quem está olhando
│   ├─ input:  Input.dispatchMouseEvent / dispatchKeyEvent / insertText
│   ├─ painel: leitura passiva do WebSocket do jogo (mesma contabilidade do app, src-tauri/src/domain/farm.rs)
│   └─ vigia de memória: recarrega a conta que passar do teto
└─ Caddy (opcional): HTTPS automático no seu domínio
```

## Deploy

Pré-requisitos: Docker com o plugin Compose, e um domínio (ou subdomínio) com registro A apontando para o IP da VPS.

```bash
git clone https://github.com/HugoGsilva/pokemon.git pokegrid && cd pokegrid
cp server/pokegrid.conf.example server/pokegrid.conf
nano server/pokegrid.conf          # PG_PASSWORD (12+ caracteres), PG_NAMES, DOMAIN

# Com o Caddy (portas 80/443 livres na VPS):
docker compose -f server/docker-compose.yml --profile caddy up -d --build

# Ou sem o Caddy, se a VPS já tem um proxy (nginx, Traefik, Cloudflare Tunnel...):
docker compose -f server/docker-compose.yml up -d --build
# e aponte o proxy para http://127.0.0.1:8080. Ele precisa repassar o WebSocket em /ws e manter o Host original
# (no nginx: proxy_set_header Host $host; Upgrade/Connection para o /ws; X-Forwarded-Proto $scheme)
```

O primeiro build leva alguns minutos, porque compila o servidor em Rust. Para acompanhar:

```bash
docker compose -f server/docker-compose.yml logs -f pokegrid
docker stats pokegrid                # RAM/CPU do container
```

Para atualizar: `git pull` e rode o mesmo `up -d --build`. O login das contas e o histórico ficam no volume
`pokegrid_data`.

### Primeiro acesso

1. Abra `https://seu-dominio`. O navegador pede usuário e senha: são o `PG_USER` e o `PG_PASSWORD` do `pokegrid.conf`.
2. Cada quadro mostra a tela de login do jogo. Amplie uma conta (botões **1–4**), clique no campo e digite. No
   celular, use o botão **⌨** para abrir o teclado.
3. Se o login abrir uma janela (outro site), ela aparece no lugar da conta até fechar (o selo mostra "janela").

> **Atenção:** não deixe a mesma conta logada no PokeGrid do PC e na VPS ao mesmo tempo. Uma derruba a outra.

## Uso

- **Grade / 1–4:** todas as contas, ou uma ampliada (com mais quadros por segundo e mais qualidade).
- **Clique / toque:** vai para a conta. Na grade só conta o clique e o arraste; com a conta ampliada o mouse
  também passa por cima (hover). No celular, arrastar rola a página do jogo quando a conta está ampliada.
- **Teclado:** vai para a conta selecionada (borda azul). Ctrl+V cola o texto da sua área de transferência.
  F5, Ctrl+R, Ctrl+W e afins continuam no seu navegador.
- **Painel:** o painel do Modo Eco. Com ele aberto, as telas não transmitem.
- **Recarregar:** dá F5 na conta selecionada, lá na VPS.
- O selo de cada conta mostra a RAM dela. Passe o mouse para ver as recargas, os reinícios e o último motivo do
  vigia.

## Memória (para caber em ~4–5 GB)

Medição no PC (WebView2): logo depois do F5, a página de uma conta ocupa cerca de 0,3–0,4 GB, mas cresce para
1–2 GB em 20 h. O jogo vaza memória, e um F5 zera esse vazamento. Por isso:

| Camada | O que faz |
|---|---|
| `--disable-gpu`, tela 1280×720, escala 1 | Sem processo de GPU guardando texturas, desenho em software |
| Chromium enxuto | Sem extensões, tradução, sync, back-forward cache e afins; cache de disco de 64 MB |
| Vigia de memória | A cada 30 s lê a RAM (PSS) de cada conta. Recarrega a maior que passou de `PG_MEM_ACCOUNT_MB` (900), ou a maior quando a soma passa de `PG_MEM_TOTAL_MB` (4300). Uma recarga por vez, no máximo a cada 2,5 min |
| Hora certa | Numa hunt, espera o próximo abate (fim da luta) por até 90 s antes de recarregar. Se a soma passar 15 % do teto, recarrega na hora |
| Escalada | Recarregou e em menos de 10 min passou do teto de novo: o vazamento não é da página, e o vigia reinicia o Chromium daquela conta (o perfil e o login continuam) |
| `PG_JS_HEAP_MB` (768) | Limite do heap de JS por página: um vazamento rápido derruba só a aba, e o servidor a reabre |
| `mem_limit: 5g` | Teto do container inteiro: o resto da VPS nunca fica sem RAM |
| Vídeo sob demanda | Ninguém olhando = nenhum quadro é gerado nem enviado |

**Swap comprimida (recomendado).** A zram é swap dentro da própria RAM, comprimida. Páginas paradas do Chromium
comprimem de 2 a 3 vezes, e ela serve de colchão para picos. No Ubuntu/Debian da VPS:

```bash
sudo apt install zram-tools
printf 'ALGO=zstd\nPERCENT=25\n' | sudo tee /etc/default/zramswap
sudo systemctl restart zramswap && swapon --show
```

**Ajustes.** Os valores ficam no `pokegrid.conf`. Com a VPS apertada, baixe `PG_MEM_ACCOUNT_MB` para 700. Para
quadros mais leves, use `PG_JPEG_QUALITY=40` e `PG_FPS=1`. Depois de mudar, rode `docker compose ... up -d` de
novo.

**Banda.** Com a grade aberta, ~0,3–0,6 Mbps. Com a conta ampliada, um pouco mais. Com o painel aberto ou a aba
escondida, zero.

## Segurança

- A página controla as suas contas. Use uma senha longa (`PG_PASSWORD`, mínimo de 12 caracteres) e sempre HTTPS
  (Caddy ou o seu proxy). Sem senha, o servidor não sobe.
- Depois de 10 senhas erradas num minuto, o login fica bloqueado até o minuto acabar. Cliques, recargas e "Zerar"
  só são aceitos vindos da própria página (o servidor confere a origem do pedido).
- O servidor só abre o site do jogo. Ele roda como usuário comum dentro do container e só publica a porta
  `127.0.0.1:8080`, que só é acessível de dentro da VPS.
- O Chromium roda com `--no-sandbox`, porque o sandbox dele precisa de privilégios que o container não tem. O
  isolamento fica com o Docker.

## Regras do jogo

O servidor não automatiza nada: nada é injetado na página e nenhuma ação acontece sozinha. Ele mostra a tela,
repassa os seus cliques e lê, de forma passiva, as mensagens que o jogo já manda (para o painel). As recargas do
vigia equivalem a apertar F5. Mesmo assim, as contas ficam 24/7 num IP de datacenter, então vale confirmar com a
staff que não há problema. O limite de 4 contas por jogador continua valendo (`PG_ACCOUNTS` vai no máximo a 4).

## Variáveis (`server/pokegrid.conf`)

| Variável | Padrão | |
|---|---|---|
| `PG_PASSWORD` | — | Obrigatória (12+ caracteres) |
| `PG_USER` | `pokegrid` | Usuário do login |
| `PG_NAMES` | `Conta 1,…` | Nomes na página e no painel |
| `DOMAIN` | — | Domínio do Caddy |
| `PG_ACCOUNTS` | `4` | Contas (1–4) |
| `PG_VIEWPORT` | `1280x720` | Tamanho da página do jogo |
| `PG_FPS` / `PG_FPS_FOCUS` | `2` / `5` | Quadros por segundo na grade e com a conta ampliada |
| `PG_JPEG_QUALITY` | `50` | Qualidade do JPEG (a conta ampliada usa +10) |
| `PG_MEM_ACCOUNT_MB` | `900` | Teto por conta |
| `PG_MEM_TOTAL_MB` | `4300` | Teto das contas somadas |
| `PG_JS_HEAP_MB` | `768` | Heap de JS por página |
| `PG_CHROME` | automático | Caminho do Chromium (fora do Docker) |
| `PG_NO_AUTH` | — | `1` desliga a senha (só para teste local, nunca na internet) |

## Problemas comuns

- **"Defina PG_PASSWORD" nos logs:** falta o `pokegrid.conf`, ou a senha nele tem menos de 12 caracteres.
- **Conta presa em "reiniciando…":** veja `docker compose ... logs pokegrid`. O motivo também aparece no selo.
- **Captcha ou bloqueio na tela de login:** pode ser o IP de datacenter. Resolva pela própria tela (clique e
  digite), ou fale com a staff.
- **Certificado não sai:** confira se o DNS do domínio já aponta para a VPS e se as portas 80/443 estão abertas
  no firewall.
