//! Terminal QR rendering.
//!
//! QR modules are packed two rows per terminal cell with Unicode half blocks:
//! `█` for a dark/dark pair, `▀` for dark/light, `▄` for light/dark and a
//! space for light/light. Every returned row has the same display width.

use qrcode::{Color, EcLevel, QrCode};

/// Renders `data` as a QR code with a `quiet_zone`-module light border.
///
/// Returns one string per terminal row, or `None` when the data cannot be
/// encoded (for example because it exceeds the maximum QR payload).
pub fn render(data: &str, quiet_zone: usize) -> Option<Vec<String>> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M).ok()?;
    let modules = code.width();
    let colors = code.to_colors();
    let total = modules + quiet_zone * 2;

    let is_dark = |x: usize, y: usize| -> bool {
        let Some(module_x) = x.checked_sub(quiet_zone) else {
            return false;
        };
        let Some(module_y) = y.checked_sub(quiet_zone) else {
            return false;
        };
        if module_x >= modules || module_y >= modules {
            return false;
        }
        colors[module_y * modules + module_x] != Color::Light
    };

    let mut rows = Vec::with_capacity(total.div_ceil(2));
    for y in (0..total).step_by(2) {
        let mut row = String::with_capacity(total);
        for x in 0..total {
            let top = is_dark(x, y);
            let bottom = y + 1 < total && is_dark(x, y + 1);
            row.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        rows.push(row);
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_uniformly_wide_rows() {
        let rows = render("tg://login?token=gramterm-test", 2).expect("QR should encode");
        assert!(!rows.is_empty());

        let width = rows[0].chars().count();
        assert!(width > 0);
        assert!(rows.iter().all(|row| row.chars().count() == width));

        // Two module rows are packed into one terminal row, so the rendered
        // grid must stay square (an odd module count leaves one spare row).
        let height = rows.len() * 2;
        assert!(
            height == width || height == width + 1,
            "{height} vs {width}"
        );

        // The quiet zone is light: the first row and first column are spaces.
        assert!(rows[0].chars().all(|c| c == ' '));
        assert!(rows.iter().all(|row| row.starts_with(' ')));
        assert!(rows.iter().all(|row| row.ends_with(' ')));
    }

    #[test]
    fn rejects_unencodable_payloads() {
        assert!(render(&"x".repeat(10_000), 1).is_none());
    }
}
