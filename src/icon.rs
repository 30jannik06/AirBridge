// Fenster-Icon, zur Laufzeit gezeichnet (keine Bilddatei nötig): blauer Verlauf mit Bildschirm und AirPlay-Dreieck.
// Jeder Pixel wird 4x4-fach abgetastet, damit die Kanten glatt sind.

/// Abstand zum Rand eines Rechtecks mit runden Ecken (negativ = innen).
fn sdf_round_rect(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let dx = ((x - cx).abs() - (hw - r)).max(0.0);
    let dy = ((y - cy).abs() - (hh - r)).max(0.0);
    (dx * dx + dy * dy).sqrt() - r
}

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let side = |p1: (f32, f32), p2: (f32, f32)| {
        (p.0 - p2.0) * (p1.1 - p2.1) - (p1.0 - p2.0) * (p.1 - p2.1)
    };
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Liefert RGBA-Bytes (gerade Alpha) für ein quadratisches Icon.
pub fn rgba(size: usize) -> Vec<u8> {
    const SS: usize = 4;
    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let (mut r, mut g, mut b, mut hits) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if sdf_round_rect(x, y, 0.5, 0.5, 0.5, 0.5, 0.22) > 0.0 {
                        continue; // außerhalb des abgerundeten Quadrats
                    }
                    let screen = sdf_round_rect(x, y, 0.5, 0.40, 0.30, 0.22, 0.06) <= 0.0
                        && sdf_round_rect(x, y, 0.5, 0.40, 0.23, 0.15, 0.03) > 0.0;
                    let triangle = in_triangle((x, y), (0.5, 0.50), (0.33, 0.80), (0.67, 0.80));
                    let (cr, cg, cb) = if screen || triangle {
                        (255.0, 255.0, 255.0)
                    } else {
                        let t = (x + y) / 2.0; // Verlauf diagonal: Blau -> Violett
                        (47.0 + 91.0 * t, 128.0 - 53.0 * t, 255.0)
                    };
                    r += cr;
                    g += cg;
                    b += cb;
                    hits += 1.0;
                }
            }
            if hits > 0.0 {
                let i = (py * size + px) * 4;
                out[i] = (r / hits) as u8;
                out[i + 1] = (g / hits) as u8;
                out[i + 2] = (b / hits) as u8;
                out[i + 3] = (hits / (SS * SS) as f32 * 255.0) as u8;
            }
        }
    }
    out
}
