use tauri::{LogicalPosition, LogicalSize};

pub const HEADER_HEIGHT: f64 = 44.0;

#[derive(Debug, Clone, Copy)]
pub struct QuadrantBounds {
    pub position: LogicalPosition<f64>,
    pub size: LogicalSize<f64>,
}

/// Calcula a posição e o tamanho dos 4 quadrantes descontando a barra superior
pub fn calculate_quadrants(window_width: f64, window_height: f64) -> [QuadrantBounds; 4] {
    let available_height = (window_height - HEADER_HEIGHT).max(100.0);
    let half_width = window_width / 2.0;
    let half_height = available_height / 2.0;

    [
        // Quadrante 1: Top-Left
        QuadrantBounds {
            position: LogicalPosition::new(0.0, HEADER_HEIGHT),
            size: LogicalSize::new(half_width, half_height),
        },
        // Quadrante 2: Top-Right
        QuadrantBounds {
            position: LogicalPosition::new(half_width, HEADER_HEIGHT),
            size: LogicalSize::new(half_width, half_height),
        },
        // Quadrante 3: Bottom-Left
        QuadrantBounds {
            position: LogicalPosition::new(0.0, HEADER_HEIGHT + half_height),
            size: LogicalSize::new(half_width, half_height),
        },
        // Quadrante 4: Bottom-Right
        QuadrantBounds {
            position: LogicalPosition::new(half_width, HEADER_HEIGHT + half_height),
            size: LogicalSize::new(half_width, half_height),
        },
    ]
}

/// Modo Foco: a conta escolhida ocupa toda a área abaixo da barra, menos o drawer quando ele está aberto
pub fn calculate_focus_bounds(window_width: f64, window_height: f64, drawer_open: bool) -> QuadrantBounds {
    let width = if drawer_open {
        window_width - drawer_width(window_width)
    } else {
        window_width
    };
    QuadrantBounds {
        position: LogicalPosition::new(0.0, HEADER_HEIGHT),
        size: LogicalSize::new(width, (window_height - HEADER_HEIGHT).max(100.0)),
    }
}

/// Contas fora de foco: mesmo tamanho do quadrante, mas fora da área visível da janela.
/// Seguem visíveis para o WebView2 (sem `hide`), então o jogo roda exatamente como na grade
pub fn calculate_offscreen_bounds(quadrant: QuadrantBounds) -> QuadrantBounds {
    QuadrantBounds {
        position: LogicalPosition::new(-quadrant.size.width - 10.0, HEADER_HEIGHT),
        size: quadrant.size,
    }
}

/// Tamanho das lives da Twitch fora do drawer: em janelas estreitas (ou 1x1) a Twitch recolhe o chat,
/// e o bônus do jogo só conta quem está com o chat carregado
pub const TWITCH_GHOST_SIZE: (f64, f64) = (1280.0, 720.0);

/// Lives da Twitch em Ghost: tamanho de desktop, mas posicionadas fora da área visível da janela
/// (a webview filha é recortada pela janela principal, então nada aparece na tela)
pub fn calculate_twitch_ghost_bounds() -> QuadrantBounds {
    let (w, h) = TWITCH_GHOST_SIZE;
    QuadrantBounds {
        position: LogicalPosition::new(-w - 10.0, 0.0),
        size: LogicalSize::new(w, h),
    }
}

fn drawer_width(window_width: f64) -> f64 {
    380.0_f64.min(window_width * 0.4)
}

/// Calcula a geometria do drawer lateral (Twitch/Discord): aberto (painel de 380px) ou Ghost (1x1 no canto)
pub fn calculate_drawer_bounds(window_width: f64, window_height: f64, is_open: bool) -> QuadrantBounds {
    if is_open {
        let drawer_width = drawer_width(window_width);
        QuadrantBounds {
            position: LogicalPosition::new(window_width - drawer_width, HEADER_HEIGHT),
            size: LogicalSize::new(drawer_width, (window_height - HEADER_HEIGHT).max(100.0)),
        }
    } else {
        // Modo Ghost: 1x1 no canto inferior direito
        QuadrantBounds {
            position: LogicalPosition::new(window_width - 1.0, window_height - 1.0),
            size: LogicalSize::new(1.0, 1.0),
        }
    }
}
