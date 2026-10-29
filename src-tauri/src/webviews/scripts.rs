// Scripts injetados SOMENTE na webview da Twitch. As webviews do jogo não recebem script algum:
// as regras do Poke Idle World proíbem scripts/automações sem permissão da staff.

/// Espera `window.__PG_TWITCH_DIRECTORY` definido antes (injetado pelo Rust)
pub const TWITCH_GHOST_SCRIPT: &str = r#"
(function() {
  const DIRECTORY_URL = window.__PG_TWITCH_DIRECTORY || 'https://www.twitch.tv/directory';
  const INVENTORY_PATH = '/drops/inventory';
  const RETURN_KEY = '__pgReturnUrl';
  const LAST_CLAIM_KEY = '__pgLastDropsCheck';
  const CLAIM_EVERY_MS = 15 * 60 * 1000;
  const NON_CHANNEL = /^\/(directory|drops|settings|search|downloads|videos|inventory|subscriptions|wallet|friends|p|u)(\/|$)/;

  window.__pgGhost = true;

  try {
    // Forçar 160p30 e mudo no localStorage da Twitch antes do player iniciar
    localStorage.setItem('video-quality', JSON.stringify({ default: '160p30' }));
    localStorage.setItem('video-muted', JSON.stringify({ default: true }));
    localStorage.setItem('mature', 'true');
  } catch (e) {}

  // O chat NÃO é bloqueado: com o WebSocket negado a Twitch reconecta e remonta o chat em loop
  // (~100x/min), vazando memória até o renderer morrer com "Out of Memory". No Ghost ele só fica oculto.

  // Ghost Mode 1px x 1px: mantem decodificação ativa no spade.twitch.tv sem consumir GPU
  window.applyTwitchGhostMode = function(isGhost) {
    window.__pgGhost = isGhost;
    let s = document.getElementById('twitch-ghost-style');
    if (isGhost) {
      if (!s) {
        s = document.createElement('style');
        s.id = 'twitch-ghost-style';
        s.textContent = `
          /* Esconder elementos periféricos pesados */
          nav, aside, footer, [data-a-target="chat-sidebar"], [data-a-target="left-nav"], .chat-room {
            display: none !important;
          }
          /* Reduzir container de vídeo para 1px com opacidade mínima (mantem decodificação para Drops) */
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
    } else if (s) {
      s.remove();
    }
  };

  function isChannelPage() {
    const p = location.pathname;
    return /^\/[A-Za-z0-9_]+\/?$/.test(p) && !NON_CHANNEL.test(p);
  }

  function goTo(url) {
    if (location.href !== url) location.href = url;
  }

  // Na página de inventário: resgata drops prontos e, se fomos nós que viemos, volta para a live
  if (location.pathname.indexOf(INVENTORY_PATH) === 0) {
    setTimeout(function() {
      Array.from(document.querySelectorAll('button'))
        .filter(function(b) { return /^(claim( now)?|resgatar( agora)?)$/i.test((b.textContent || '').trim()); })
        .forEach(function(b) {
          console.log('[PokeGrid] Resgatando Drop concluído automaticamente!');
          b.click();
        });

      let back = null;
      try {
        back = sessionStorage.getItem(RETURN_KEY);
        sessionStorage.removeItem(RETURN_KEY);
      } catch (e) {}
      if (back) setTimeout(function() { goTo(back); }, 5000);
    }, 10000);
  }

  // A cada 15 min, vai ao inventário resgatar drops (o botão não existe na página da live)
  function maybeVisitInventory() {
    let last = 0;
    try { last = Number(localStorage.getItem(LAST_CLAIM_KEY) || 0); } catch (e) { return; }
    if (!last) {
      try { localStorage.setItem(LAST_CLAIM_KEY, String(Date.now())); } catch (e) {}
      return;
    }
    if (Date.now() - last < CLAIM_EVERY_MS) return;
    try {
      localStorage.setItem(LAST_CLAIM_KEY, String(Date.now()));
      sessionStorage.setItem(RETURN_KEY, location.href);
    } catch (e) { return; }
    goTo(location.origin + INVENTORY_PATH);
  }

  // Watchdog (só em Ghost, para não atrapalhar quem navega pelo drawer aberto)
  let stalled = 0;
  setInterval(function() {
    if (!window.__pgGhost) return;
    const path = location.pathname;
    if (path.indexOf(INVENTORY_PATH) === 0) return;

    // No diretório: entra na primeira live listada
    if (path.indexOf('/directory/') === 0) {
      const card = document.querySelector('a[data-a-target="preview-card-image-link"]');
      if (card && card.href) goTo(card.href);
      return;
    }
    if (!isChannelPage()) return;

    const v = document.querySelector('video');
    if (v) {
      v.muted = true;
      if (v.paused) v.play().catch(function() {});
    }
    stalled = (!v || v.paused || v.readyState < 2) ? stalled + 1 : 0;
    // ~2 min sem vídeo: live encerrou ou travou → volta ao diretório e escolhe outra
    if (stalled >= 6) {
      stalled = 0;
      goTo(DIRECTORY_URL);
      return;
    }

    // Bônus de pontos do canal
    const bonus = document.querySelector('.claimable-bonus__icon');
    const bonusBtn = bonus && bonus.closest('button');
    if (bonusBtn) bonusBtn.click();

    maybeVisitInventory();
  }, 20000);

  // Inicia em modo Ghost por padrão (o Rust reaplica o estado real do drawer ao fim do load)
  window.addEventListener('DOMContentLoaded', function() {
    window.applyTwitchGhostMode(window.__pgGhost);
  });
})();
"#;
