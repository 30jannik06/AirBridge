// Window icon, drawn at runtime (no image file needed): blue gradient with a screen and an AirPlay triangle.
// Every pixel is sampled 4x4 times so the edges are smooth.

/// Distance to the edge of a rectangle with rounded corners (negative = inside).
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

/// Returns RGBA bytes (straight alpha) for a square icon.
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
                        continue; // outside the rounded square
                    }
                    let screen = sdf_round_rect(x, y, 0.5, 0.40, 0.30, 0.22, 0.06) <= 0.0
                        && sdf_round_rect(x, y, 0.5, 0.40, 0.23, 0.15, 0.03) > 0.0;
                    let triangle = in_triangle((x, y), (0.5, 0.50), (0.33, 0.80), (0.67, 0.80));
                    let (cr, cg, cb) = if screen || triangle {
                        (255.0, 255.0, 255.0)
                    } else {
                        let t = (x + y) / 2.0; // diagonal gradient: blue -> violet
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

/// Builds an .ico file (several sizes, PNG-compressed) from the drawn icon.
/// Only needed to create `assets/airbridge.ico` (`airbridge --save-ico <file>`).
pub fn ico() -> anyhow::Result<Vec<u8>> {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    const SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

    let mut images: Vec<Vec<u8>> = Vec::new();
    for size in SIZES {
        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(
            &rgba(size as usize),
            size,
            size,
            ExtendedColorType::Rgba8,
        )?;
        images.push(png);
    }

    let mut out = vec![0, 0, 1, 0]; // reserved, type 1 = icon
    out.extend((SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (size, png) in SIZES.iter().zip(&images) {
        out.push(if *size >= 256 { 0 } else { *size as u8 }); // width (0 = 256)
        out.push(if *size >= 256 { 0 } else { *size as u8 }); // height
        out.extend([0, 0]); // colour palette, reserved
        out.extend(1u16.to_le_bytes()); // colour planes
        out.extend(32u16.to_le_bytes()); // bits per pixel
        out.extend((png.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in images {
        out.extend(png);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_has_a_valid_directory() {
        let ico = ico().unwrap();
        assert_eq!(&ico[..4], &[0, 0, 1, 0]);
        let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
        assert_eq!(count, 6);
        // Every entry points to a PNG inside the file.
        for i in 0..count {
            let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
            let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
            let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
            assert_eq!(&ico[off..off + 8], b"\x89PNG\r\n\x1a\n");
            assert!(off + len <= ico.len());
        }
    }
}
