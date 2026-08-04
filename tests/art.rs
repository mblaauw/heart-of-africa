//! Phase 6 art pipeline tests: `hoatool gen-art` is deterministic, and the
//! committed atlas agrees with the loader's biome table (one 32px row per
//! biome, two variant columns).

mod common;

use std::path::Path;

/// gen-art is deterministic: two runs into fresh directories produce
/// byte-identical files.
#[test]
fn gen_art_is_deterministic() {
    let out_a = std::env::temp_dir().join(format!("hoa-art-a-{}", std::process::id()));
    let out_b = std::env::temp_dir().join(format!("hoa-art-b-{}", std::process::id()));
    run_gen_art(&out_a);
    run_gen_art(&out_b);
    for rel in [
        "art/biome_tiles.png",
        "art/player_marker.png",
        "art/settlement_marker.png",
        "art/tomb_marker.png",
        "audio/wind.wav",
    ] {
        let a = std::fs::read(out_a.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let b = std::fs::read(out_b.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(a, b, "{rel} differs between runs");
    }
    let _ = std::fs::remove_dir_all(&out_a);
    let _ = std::fs::remove_dir_all(&out_b);
}

fn run_gen_art(out: &Path) {
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_hoatool"))
        .args(["gen-art"])
        .arg(common::content_dir().join("world.toml"))
        .arg(out)
        .status()
        .expect("hoatool binary runs");
    assert!(status.success(), "gen-art exited nonzero");
}

/// The committed atlas matches the loaded biome table: one 32px row per
/// biome (in table order), two 32px variant columns, and each row's base
/// color is the biome's color (modulo the seeded ±8% texture).
#[test]
fn atlas_rows_match_loader_biome_order() {
    let world = heart_of_africa::content::load::load_world(common::content_dir()).unwrap();
    let atlas = image::open(common::assets_dir().join("art/biome_tiles.png"))
        .expect("committed atlas loads")
        .to_rgba8();
    let rows = world.grid.biome_count() as u32;
    assert_eq!(atlas.width(), 64, "two variant columns");
    assert_eq!(atlas.height(), rows * 32, "one row per biome");

    for row in 0..rows {
        let biome = world
            .grid
            .biome_by_index(row as u16)
            .expect("biome by index");
        let px = atlas.get_pixel(16, row * 32 + 16); // center of variant 0
        let (er, eg, eb) = biome.color;
        let close = |a: u8, b: u8| a.abs_diff(b) <= 24;
        assert!(
            close(px[0], er) && close(px[1], eg) && close(px[2], eb),
            "row {row} ({}) base color {px:?} diverges from {:?}",
            biome.id,
            biome.color
        );
    }
}
