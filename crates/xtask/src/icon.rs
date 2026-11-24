//! The app's icon, drawn in code: a rounded square in the app's accent colour with a white slide and a small play triangle.
//! An original mark; every platform's icon comes from here.

use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

/// The sizes written as PNGs.
pub const PNG_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// The sizes held in the `.ico`.
pub const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

/// The app's default accent colour (the toolkit theme's `accent`).
const ACCENT: (u8, u8, u8) = (0x25, 0x63, 0xeb);

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    // A cubic arc's control distance for a quarter circle.
    let k = r * 0.552_285;
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(x + w - r, y);
    b.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    b.line_to(x + w, y + h - r);
    b.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    b.line_to(x + r, y + h);
    b.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    b.close();
    b.finish().expect("a rounded rectangle has a size")
}

fn paint(rgba: (u8, u8, u8, u8)) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(rgba.0, rgba.1, rgba.2, rgba.3));
    p.anti_alias = true;
    p
}

/// The icon at `size` x `size` pixels.
pub fn draw_icon(size: u32) -> Pixmap {
    let mut pm = Pixmap::new(size, size).expect("an icon size is not zero");
    let s = size as f32;
    let id = Transform::identity();
    let accent = paint((ACCENT.0, ACCENT.1, ACCENT.2, 255));
    let white = paint((255, 255, 255, 255));

    // The tile.
    pm.fill_path(&rounded_rect(0.0, 0.0, s, s, s * 0.22), &accent, FillRule::Winding, id, None);
    // The slide: a white 16:10 sheet, a little above the middle.
    let (sw, sh) = (s * 0.68, s * 0.68 * 0.625);
    let (sx, sy) = ((s - sw) / 2.0, s * 0.5 - sh * 0.5 - s * 0.03);
    pm.fill_path(&rounded_rect(sx, sy, sw, sh, s * 0.06), &white, FillRule::Winding, id, None);
    // The play triangle, in the accent colour, centred on the slide (a triangle's visual centre sits a little right of its box).
    let t = sh * 0.46;
    let (cx, cy) = (sx + sw / 2.0 + t * 0.08, sy + sh / 2.0);
    let mut b = PathBuilder::new();
    b.move_to(cx - t * 0.4, cy - t * 0.5);
    b.line_to(cx + t * 0.5, cy);
    b.line_to(cx - t * 0.4, cy + t * 0.5);
    b.close();
    pm.fill_path(&b.finish().expect("a triangle has a size"), &accent, FillRule::Winding, id, None);
    // The stand: a short foot under the slide, so it reads as a screen.
    let foot_w = sw * 0.34;
    pm.fill_path(
        &rounded_rect((s - foot_w) / 2.0, sy + sh + s * 0.045, foot_w, s * 0.04, s * 0.02),
        &white,
        FillRule::Winding,
        id,
        None,
    );
    pm
}

/// The icon at `size`, as PNG bytes.
pub fn icon_png(size: u32) -> Vec<u8> {
    draw_icon(size).encode_png().expect("a drawn icon encodes")
}

/// An `.ico` holding the given PNGs (PNG-in-ICO); a size of 256 is stored as 0.
pub fn write_ico(pngs: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, png) in pngs {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in pngs {
        out.extend_from_slice(png);
    }
    out
}

/// The `.ico` of every size it holds.
pub fn icon_ico() -> Vec<u8> {
    let pngs: Vec<(u32, Vec<u8>)> = ICO_SIZES.iter().map(|&s| (s, icon_png(s))).collect();
    write_ico(&pngs)
}

/// Writes `icon-<size>.png` for every size and `stagepack-studio.ico` into `dir`.
pub fn write_icons(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    for size in PNG_SIZES {
        let path = dir.join(format!("icon-{size}.png"));
        std::fs::write(&path, icon_png(size)).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    let path = dir.join("stagepack-studio.ico");
    std::fs::write(&path, icon_ico()).map_err(|e| format!("can't write {}: {e}", path.display()))
}
