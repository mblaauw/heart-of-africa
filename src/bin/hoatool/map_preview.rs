//! `hoatool map preview <content-dir> [out.png]` — render a
//! geography preview PNG showing settlements, rivers, routes,
//! regions, and landmarks on the biome map.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// Run `hoatool map preview <content-dir> [out.png]`.
pub fn run_map_preview(args: &[String]) -> Result<()> {
    let content_dir = match args.first() {
        Some(d) => d,
        None => bail!("usage: hoatool map preview <content-dir> [out.png]"),
    };
    let out_png = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("geography_preview.png");

    let world = heart_of_africa::content::load::load_world(Path::new(content_dir))
        .context("loading world")?;

    let (width, height) = (world.width, world.height);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

    // Base layer: biome colors
    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = world
                .grid
                .biome_at(x, y)
                .map(|b| b.color)
                .unwrap_or((0, 0, 0));
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }

    // Draw rivers as blue lines
    for river in &world.rivers {
        for &(tx, ty) in &river.points {
            if tx < width && ty < height {
                let idx = (ty as usize * width as usize + tx as usize) * 4;
                rgba[idx] = 30;
                rgba[idx + 1] = 100;
                rgba[idx + 2] = 200;
                rgba[idx + 3] = 255;
            }
        }
    }

    // Draw routes as yellow lines
    for route in &world.routes {
        for &(tx, ty) in &route.tiles {
            if tx < width && ty < height {
                let idx = (ty as usize * width as usize + tx as usize) * 4;
                rgba[idx] = 255;
                rgba[idx + 1] = 200;
                rgba[idx + 2] = 30;
                rgba[idx + 3] = 255;
            }
        }
    }

    // Draw settlements as white dots
    for s in &world.settlements {
        if s.x < width && s.y < height {
            draw_dot(&mut rgba, width, s.x, s.y, 255, 255, 255, 255);
        }
    }

    // Draw landmarks as green dots
    for lm in &world.landmarks {
        let (tx, ty) = km_to_tile(lm.point, &world);
        if tx < width && ty < height {
            draw_dot(&mut rgba, width, tx, ty, 30, 200, 30, 255);
        }
    }

    // Draw regions as red outlines
    for region in &world.regions {
        for point in &region.polygon.ring {
            let (tx, ty) = km_to_tile(*point, &world);
            if tx < width && ty < height {
                draw_dot(&mut rgba, width, tx, ty, 200, 30, 30, 255);
            }
        }
    }

    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| anyhow::anyhow!("failed to build {}×{} image", width, height))?;
    img.save(out_png)
        .with_context(|| format!("writing {}", out_png))?;

    println!("map preview: wrote {}", out_png);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_dot(rgba: &mut [u8], width: u32, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    let idx = (y as usize * width as usize + x as usize) * 4;
    if idx + 3 < rgba.len() {
        rgba[idx] = r;
        rgba[idx + 1] = g;
        rgba[idx + 2] = b;
        rgba[idx + 3] = a;
    }
}

fn km_to_tile(
    p: heart_of_africa::world::geo::KmPoint,
    world: &heart_of_africa::world::World,
) -> (u32, u32) {
    let x = (p.x / heart_of_africa::world::KM_PER_CELL).floor().max(0.0) as u32;
    let y = (p.y / heart_of_africa::world::KM_PER_CELL).floor().max(0.0) as u32;
    (
        x.min(world.width.saturating_sub(1)),
        y.min(world.height.saturating_sub(1)),
    )
}
