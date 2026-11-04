// Scripts injetados SOMENTE nas webviews da Twitch. As webviews do jogo não recebem script algum:
// as regras do Poke Idle World proíbem scripts/automações sem permissão da staff.

/// Espera `window.__PG_TWITCH_SLOT`, `__PG_TWITCH_LIVES`, `__PG_TWITCH_IDLE` e `__PG_TWITCH_CHANNELS` definidos antes
/// (injetados pelo Rust; a lista também é reenviada a cada load e quando o usuário a altera).
///
/// O bônus de live do jogo vem do bot do jogo, moderador dos canais oficiais, que lê quem está CONECTADO AO CHAT
/// logado. Por isso cada live abre a página completa do canal (vídeo + chat), e não só o player.
pub const TWITCH_GHOST_SCRIPT: &str = r#"
(function() {
  // Cada webview é um "slot" (1..5); o slot 1 é o que aparece no drawer
  const SLOT = window.__PG_TWITCH_SLOT || 1;
  const MAX_LIVES = window.__PG_TWITCH_LIVES || 5;
  const IDLE_URL = window.__PG_TWITCH_IDLE || 'https://www.twitch.tv/robots.txt';
  const WWW = 'https://www.twitch.tv';
  // Mesma consulta pública que o site da Twitch faz; o Client-Id é o do próprio site (não é segredo)
  const GQL_URL = 'https://gql.twitch.tv/gql';
  const CLIENT_ID = 'kimne78kx3ncx6brgo4mv6wki5h1ko';
  const CHECK_EVERY_MS = 60 * 1000;
  // Canal de cada slot no localStorage de www.twitch.tv (compartilhado pelas webviews do mesmo perfil),
  // para duas lives nunca ficarem no mesmo canal
  const SLOTS_KEY = '__pgSlots';
  const SLOT_TTL_MS = 150 * 1000;
  const NON_CHANNEL = /^\/(directory|drops|settings|search|downloads|videos|inventory|subscriptions|wallet|friends|popout|robots\.txt|p|u)(\/|$)/;

  if (location.hostname !== 'www.twitch.tv') return;

  window.__pgGhost = true;

  try {
    // Forçar 160p30 e mudo no localStorage da Twitch antes do player iniciar
    localStorage.setItem('video-quality', JSON.stringify({ default: '160p30' }));
    localStorage.setItem('video-muted', JSON.stringify({ default: true }));
    localStorage.setItem('mature', 'true');
  } catch (e) {}

  // O chat NÃO é bloqueado nem escondido: é por ele que o jogo encontra o usuário na live.
  // (Bloquear o WebSocket do chat também fazia a Twitch reconectar em loop até o renderer morrer.)

  // Ghost Mode 1px x 1px: vídeo em 160p mudo e minúsculo, sem consumir GPU; o chat continua carregado
  window.applyTwitchGhostMode = function(isGhost) {
    window.__pgGhost = isGhost;
    let s = document.getElementById('twitch-ghost-style');
    if (isGhost) {
      if (!s) {
        s = document.createElement('style');
        s.id = 'twitch-ghost-style';
        s.textContent = `
          /* Esconder elementos periféricos (o chat fica) */
          nav, footer, [data-a-target="left-nav"], [data-a-target="side-nav-bar"] {
            display: none !important;
          }
          /* Reduzir o vídeo para 1px com opacidade mínima (continua tocando) */
          .video-player, video {
            width: 1px !important;
            height: 1px !important;
            opacity: 0.01 !important;
            pointer-events: none !important;
            position: fixed !important;
            top: 0 !important;
            left: 0 !important;
          }
        `;
        (document.head || document.documentElement).appendChild(s);
      }
      const v = document.querySelector('video');
      if (v) v.muted = true;
    } else {
      if (s) s.remove();
      // Drawer aberto numa live sem canal: mostra a Twitch (para login/navegação) em vez da página vazia
      if (location.href === IDLE_URL) goTo(WWW + '/');
    }
  };

  function goTo(url) {
    if (location.href !== url) location.href = url;
  }

  function currentChannel() {
    const p = location.pathname;
    if (!/^\/[A-Za-z0-9_]+\/?$/.test(p) || NON_CHANNEL.test(p)) return null;
    return p.replace(/^\/|\/$/g, '').toLowerCase();
  }

  function readSlots() {
    try { return JSON.parse(localStorage.getItem(SLOTS_KEY) || '{}') || {}; } catch (e) { return {}; }
  }

  function claim(ch) {
    const slots = readSlots();
    slots[SLOT] = { ch: ch, t: Date.now() };
    try { localStorage.setItem(SLOTS_KEY, JSON.stringify(slots)); } catch (e) {}
  }

  // Canal -> menor slot ativo que o ocupa (fora este)
  function othersByChannel() {
    const slots = readSlots();
    const now = Date.now();
    const owners = {};
    Object.keys(slots).forEach(function(k) {
      const s = slots[k];
      const n = Number(k);
      if (n === SLOT || !s || !s.ch || now - s.t > SLOT_TTL_MS) return;
      if (!(s.ch in owners) || n < owners[s.ch]) owners[s.ch] = n;
    });
    return owners;
  }

  // Canais da lista que estão ao vivo, na ordem da lista
  async function liveChannels(list) {
    const query = 'query{users(logins:' + JSON.stringify(list) + '){login stream{id}}}';
    const res = await fetch(GQL_URL, {
      method: 'POST',
      headers: { 'Client-Id': CLIENT_ID, 'Content-Type': 'application/json' },
      body: JSON.stringify({ query: query }),
    });
    const data = await res.json();
    const users = (data && data.data && data.data.users) || [];
    return users.filter(function(u) { return u && u.stream; }).map(function(u) { return u.login.toLowerCase(); });
  }

  let checking = false;
  async function checkLives() {
    if (!window.__pgGhost || checking) return;
    checking = true;
    try {
      const list = window.__PG_TWITCH_CHANNELS || [];
      // Os canais que devem ser assistidos: os MAX_LIVES primeiros da lista que estão ao vivo
      const wanted = (list.length ? await liveChannels(list) : []).slice(0, MAX_LIVES);
      if (!window.__pgGhost) return;

      const owners = othersByChannel();
      const cur = currentChannel();
      // Fica onde está se o canal segue ao vivo/prioritário e nenhum slot de número menor também está nele
      if (cur && wanted.indexOf(cur) >= 0 && !(owners[cur] < SLOT)) {
        claim(cur);
        return;
      }
      const free = wanted.find(function(ch) { return !(ch in owners); });
      claim(free || null);
      goTo(free ? WWW + '/' + free : IDLE_URL);
    } catch (e) {
      // Rede/Twitch fora do ar: continua onde está e tenta de novo no próximo ciclo
    } finally {
      checking = false;
    }
  }
  window.__pgCheckLives = checkLives;

  // Mantém o chat aberto e o vídeo tocando e mudo (só em Ghost, para não atrapalhar quem usa o drawer)
  function keepWatching() {
    if (!window.__pgGhost || !currentChannel()) return;
    // A Twitch recolhe o chat em janelas estreitas e guarda essa preferência; recolhido ele não conta
    // para o bônus. Só clica se a coluna estiver de fato recolhida (o mesmo botão também recolhe).
    if (document.querySelector('.right-column--collapsed')) {
      const toggle = document.querySelector('[data-a-target="right-column__toggle-collapse-btn"]');
      if (toggle) toggle.click();
    }
    const v = document.querySelector('video');
    if (v) {
      v.muted = true;
      if (v.paused) v.play().catch(function() {});
    }
  }
  setInterval(keepWatching, 20000);

  // Slots começam escalonados (2s entre eles) para não escolherem o mesmo canal ao mesmo tempo
  setTimeout(function() {
    checkLives();
    setInterval(checkLives, CHECK_EVERY_MS);
  }, 3000 + (SLOT - 1) * 2000);

  // Inicia em modo Ghost por padrão (o Rust reaplica o estado real do drawer ao fim do load)
  window.addEventListener('DOMContentLoaded', function() {
    window.applyTwitchGhostMode(window.__pgGhost);
  });
  // Primeira checagem do chat logo após a página da live montar (depois segue a cada 20s)
  window.addEventListener('load', function() { setTimeout(keepWatching, 5000); });
})();
"#;
