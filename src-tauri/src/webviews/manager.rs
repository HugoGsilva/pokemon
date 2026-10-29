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

/// Calcula a geometria da Twitch: Drawer aberto (painel lateral de 360px) ou Ghost (1x1 no canto)
pub fn calculate_twitch_bounds(window_width: f64, window_height: f64, is_open: bool) -> QuadrantBounds {
    if is_open {
        let drawer_width = 380.0_f64.min(window_width * 0.4);
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
