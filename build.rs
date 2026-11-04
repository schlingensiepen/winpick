// =============================================================================
//  WinPick – build script
//  Version: 2026-11-04 12:10 (Europe/Berlin)
//
//  Generates the application icon from code (no binary files in the repo),
//  writes a resource script with icon, manifest and version info, and
//  compiles it into the executable via embed-resource.
// =============================================================================
use std::env;
use std::fs;
use std::path::PathBuf;

const BG: [u8; 3] = [30, 33, 40];
const ACCENT: [u8; 3] = [0, 120, 215];
const TILE: [u8; 3] = [200, 205, 215];
const SIZES: [u32; 6] = [256, 64, 48, 32, 24, 16];
const SS: u32 = 4; // supersampling per axis

fn in_round_rect(x: f64, y: f64, r: (f64, f64, f64, f64), rad: f64) -> bool {
    let (x0, y0, x1, y1) = r;
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return false;
    }
    let cx = x.max(x0 + rad).min(x1 - rad);
    let cy = y.max(y0 + rad).min(y1 - rad);
    (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad
}

/// RGBA rows, top to bottom.
fn render(s: u32) -> Vec<Vec<u8>> {
    let sf = s as f64;
    type Shape = ((f64, f64, f64, f64), f64, [u8; 3]);
    let mut shapes: Vec<Shape> = vec![((0.0, 0.0, sf, sf), sf * 0.18, BG)];
    let (m, g) = (sf * 0.16, sf * 0.08);
    let t = (sf - 2.0 * m - g) / 2.0;
    let cols = [ACCENT, TILE, TILE, TILE];
    for (k, col) in cols.iter().enumerate() {
        let (xx, yy) = ((k % 2) as f64, (k / 2) as f64);
        let x = m + xx * (t + g);
        let y = m + yy * (t + g);
        shapes.push(((x, y, x + t, y + t), t * 0.18, *col));
    }
    let n = (SS * SS) as f64;
    (0..s)
        .map(|py| {
            let mut row = Vec::with_capacity(s as usize * 4);
            for px in 0..s {
                let (mut r, mut gg, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
                for sy in 0..SS {
                    for sx in 0..SS {
                        let x = px as f64 + (sx as f64 + 0.5) / SS as f64;
                        let y = py as f64 + (sy as f64 + 0.5) / SS as f64;
                        let mut col = None;
                        for (rect, rad, c) in &shapes {
                            if in_round_rect(x, y, *rect, *rad) {
                                col = Some(*c);
                            }
                        }
                        if let Some(c) = col {
                            r += c[0] as f64;
                            gg += c[1] as f64;
                            b += c[2] as f64;
                            a += 1.0;
                        }
                    }
                }
                if a > 0.0 {
                    row.extend_from_slice(&[
                        (r / a).round() as u8,
                        (gg / a).round() as u8,
                        (b / a).round() as u8,
                        (255.0 * a / n).round() as u8,
                    ]);
                } else {
                    row.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
            row
        })
        .collect()
}

/// 32-bit BMP icon entry (BITMAPINFOHEADER + BGRA bottom-up + empty AND mask).
fn bmp_entry(s: u32, rows: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [40u32, s, s * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    for row in rows.iter().rev() {
        for px in row.chunks(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    let mask_row = s.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, (mask_row * s) as usize));
    out
}

fn make_icon() -> Vec<u8> {
    let entries: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, bmp_entry(s, &render(s)))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * entries.len() as u32;
    for (s, blob) in &entries {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += blob.len() as u32;
    }
    for (_, blob) in entries {
        out.extend_from_slice(&blob);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res/app.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    fs::write(out.join("app.ico"), make_icon()).unwrap();

    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = ver.split(['.', '-']).take(3).map(|p| p.to_string()).collect();
    parts.resize(3, "0".into());
    let num = format!("{},{},{},0", parts[0], parts[1], parts[2]);
    let esc = |p: PathBuf| p.display().to_string().replace('\\', "\\\\");
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {num}
PRODUCTVERSION {num}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Jörn Schlingensiepen"
      VALUE "FileDescription", "WinPick - window overview with title search"
      VALUE "ProductName", "WinPick"
      VALUE "FileVersion", "{ver}"
      VALUE "ProductVersion", "{ver}"
      VALUE "LegalCopyright", "Copyright 2026 Jörn Schlingensiepen, Apache License 2.0"
      VALUE "OriginalFilename", "WinPick.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
        icon = esc(out.join("app.ico")),
        manifest = esc(root.join("res").join("app.manifest")),
    );
    let rc_path = out.join("app.rc");
    fs::write(&rc_path, rc).unwrap();
    // Resource compilation needs the Windows SDK (rc.exe). On other hosts
    // (e.g. `cargo check` on Linux) the resources are skipped.
    if !env::var("HOST").unwrap_or_default().contains("windows") {
        println!("cargo:warning=non-Windows host: resources (icon, manifest, version) not embedded");
        return;
    }
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().unwrap();
}
