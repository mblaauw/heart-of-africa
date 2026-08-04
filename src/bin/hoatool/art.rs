//! Procedural art generation (DESIGN.md §5, Phase 6): the style-bible
//! tileset, marker art, and the ambient wind loop. Everything is seeded and
//! deterministic — the same command produces byte-identical files, so the
//! committed PNGs/WAV are canonical and regenerating is always safe.
//!
//! Invariants (see docs/STYLE_BIBLE.md):
//! - Atlas rows are one per biome, in alphabetical biome-id order — the
//!   same order `content::load` builds its biome table in.
//! - Tile texture is subtle value noise (±8% brightness) — muted, painterly.

use std::path::Path;

use anyhow::{Context, Result};

/// On-screen size of one map tile and of one atlas tile.
pub const TILE: u32 = 32;
/// Seeded tile variants per biome in the atlas.
pub const VARIANTS: u32 = 2;
/// Amplitude of the value-noise texture modulation.
const TEXTURE_AMOUNT: f32 = 0.08;

/// Generate all art and audio into `assets_dir` (the manifest lives there):
/// `art/biome_tiles.png`, `art/{player,settlement,tomb}_marker.png`,
/// `audio/wind.wav`. `world_toml` supplies the biome palette.
pub fn generate(world_toml: &Path, assets_dir: &Path, seed: u64) -> Result<()> {
    let colors = super::biome_colors(world_toml)?;

    let art_dir = assets_dir.join("art");
    let audio_dir = assets_dir.join("audio");
    std::fs::create_dir_all(&art_dir).with_context(|| format!("creating {}", art_dir.display()))?;
    std::fs::create_dir_all(&audio_dir)
        .with_context(|| format!("creating {}", audio_dir.display()))?;

    let (aw, ah, atlas) = biome_atlas(&colors, seed);
    write_png(&art_dir.join("biome_tiles.png"), aw, ah, atlas)?;
    for (name, kind) in [
        ("player_marker.png", Marker::Player),
        ("settlement_marker.png", Marker::Settlement),
        ("tomb_marker.png", Marker::Tomb),
    ] {
        let (mw, mh, rgba) = marker(kind);
        write_png(&art_dir.join(name), mw, mh, rgba)?;
    }

    let wind = wind_wav(seed);
    std::fs::write(audio_dir.join("wind.wav"), &wind)
        .with_context(|| format!("writing {}", audio_dir.join("wind.wav").display()))?;

    Ok(())
}

/// The biome atlas: `biomes.len()` rows × `VARIANTS` columns of 32×32 tiles.
/// Rows follow the biome ids in sorted order (BTreeMap order), matching the
/// order `content::load` builds its biome table in — the renderer's
/// `BiomeGrid::biome_index_at` row is the same index.
fn biome_atlas(
    colors: &std::collections::BTreeMap<String, (u8, u8, u8)>,
    seed: u64,
) -> (u32, u32, Vec<u8>) {
    let rows = colors.len() as u32;
    let width = TILE * VARIANTS;
    let height = TILE * rows;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    for (row, (_biome_id, (r, g, b))) in colors.iter().enumerate() {
        let (r, g, b) = (*r, *g, *b);
        for variant in 0..VARIANTS {
            for ty in 0..TILE {
                for tx in 0..TILE {
                    let x = tx + variant * TILE;
                    let y = ty + row as u32 * TILE;
                    let noise = smooth_noise(tx, ty, seed ^ row as u64, 5) * TEXTURE_AMOUNT;
                    let px = ((r as f32) * (1.0 + noise)).clamp(0.0, 255.0) as u8;
                    let py = ((g as f32) * (1.0 + noise)).clamp(0.0, 255.0) as u8;
                    let pz = ((b as f32) * (1.0 + noise)).clamp(0.0, 255.0) as u8;
                    let i = ((y * width + x) * 4) as usize;
                    rgba[i..i + 4].copy_from_slice(&[px, py, pz, 255]);
                }
            }
        }
    }
    (width, height, rgba)
}

/// Marker shapes (32×32, transparent background, dark outline).
enum Marker {
    Player,
    Settlement,
    Tomb,
}

fn marker(kind: Marker) -> (u32, u32, Vec<u8>) {
    let (fill, outline): ((u8, u8, u8), (u8, u8, u8)) = match kind {
        Marker::Player => ((0xff, 0xd6, 0x4d), (0x4a, 0x3b, 0x2a)),
        Marker::Settlement => ((0xff, 0xff, 0xff), (0x4a, 0x3b, 0x2a)),
        Marker::Tomb => ((0xd9, 0x3f, 0x33), (0x4a, 0x3b, 0x2a)),
    };
    let mut rgba = vec![0u8; (TILE * TILE * 4) as usize];
    let center = TILE as f32 / 2.0 - 0.5;
    for y in 0..TILE {
        for x in 0..TILE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let (inside, ring) = match kind {
                Marker::Player => {
                    let r = (dx * dx + dy * dy).sqrt();
                    (r <= 11.0, r > 11.0 && r <= 12.5)
                }
                Marker::Settlement => {
                    let m = dx.abs().max(dy.abs());
                    (m <= 10.0, m > 10.0 && m <= 11.5)
                }
                Marker::Tomb => {
                    let d = dx.abs() + dy.abs();
                    (d <= 14.0, d > 14.0 && d <= 16.0)
                }
            };
            let color = if inside {
                fill
            } else if ring {
                outline
            } else {
                continue;
            };
            let i = ((y * TILE + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[color.0, color.1, color.2, 255]);
        }
    }
    (TILE, TILE, rgba)
}

/// A short loop of seeded low-passed noise: wind over open country.
/// Mono 16-bit PCM, 22050 Hz, 4 seconds.
fn wind_wav(seed: u64) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 22050;
    const SECONDS: u32 = 4;
    let frames = SAMPLE_RATE * SECONDS;
    let mut pcm = Vec::with_capacity(frames as usize * 2);
    let mut low = 0.0f32;
    for i in 0..frames {
        // One-pole lowpass on white noise: soft, windy, not hissy.
        let white = value_noise(i, 0, seed);
        low = 0.985 * low + 0.015 * white;
        // Slow gust modulation from a second stream.
        let gust = 0.5 + 0.5 * value_noise(i / 128, 7, seed);
        let v = (low * gust * 0.6).clamp(-1.0, 1.0);
        pcm.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
    }
    let mut wav = wav_header(1, SAMPLE_RATE, 16, frames);
    wav.extend_from_slice(&pcm);
    wav
}

/// Minimal PCM WAV header (44 bytes) — no audio crate needed.
fn wav_header(channels: u16, sample_rate: u32, bits: u16, frames: u32) -> Vec<u8> {
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * block_align as u32;
    let data_len = frames * block_align as u32;
    let mut v = Vec::with_capacity(44);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&byte_rate.to_le_bytes());
    v.extend_from_slice(&block_align.to_le_bytes());
    v.extend_from_slice(&bits.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v
}

/// Deterministic hash-based value noise in [-1, 1] (FNV-1a over x, y, seed).
fn value_noise(x: u32, y: u32, seed: u64) -> f32 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in [seed, x as u64, y as u64]
        .iter()
        .flat_map(|v| v.to_le_bytes())
    {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let frac = ((hash >> 40) as f64) / ((1u64 << 24) as f64);
    (frac * 2.0 - 1.0) as f32
}

/// Smooth (bilinearly interpolated) value noise over a lattice of `cell` px.
fn smooth_noise(x: u32, y: u32, seed: u64, cell: u32) -> f32 {
    let cx = x / cell;
    let cy = y / cell;
    let fx = (x % cell) as f32 / cell as f32;
    let fy = (y % cell) as f32 / cell as f32;
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let fx = smooth(fx);
    let fy = smooth(fy);
    let v00 = value_noise(cx, cy, seed);
    let v10 = value_noise(cx + 1, cy, seed);
    let v01 = value_noise(cx, cy + 1, seed);
    let v11 = value_noise(cx + 1, cy + 1, seed);
    let a = v00 * (1.0 - fx) + v10 * fx;
    let b = v01 * (1.0 - fx) + v11 * fx;
    a * (1.0 - fy) + b * fy
}

fn write_png(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<()> {
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| anyhow::anyhow!("failed to build image buffer"))?;
    img.save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_bounded_and_seeded() {
        for seed in [0u64, 1, 7] {
            let n = value_noise(3, 9, seed);
            assert!((-1.0..=1.0).contains(&n));
        }
        assert_ne!(value_noise(3, 9, 1), value_noise(3, 9, 2));
        assert_eq!(value_noise(3, 9, 1), value_noise(3, 9, 1));
    }

    #[test]
    fn atlas_dimensions_follow_biome_count() {
        let colors = std::collections::BTreeMap::from([
            ("desert".to_string(), (0xd9, 0xb3, 0x6c)),
            ("water".to_string(), (0x3a, 0x6e, 0xa5)),
        ]);
        let (w, h, atlas) = biome_atlas(&colors, 0);
        // 2 biomes x 2 variants x 32px.
        assert_eq!((w, h), (TILE * 2, TILE * 2));
        assert_eq!(atlas.len(), (w * h * 4) as usize);
    }

    #[test]
    fn wav_header_is_valid_pcm() {
        let wav = wav_header(1, 22050, 16, 100);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1); // PCM
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // mono
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            22050
        );
    }

    #[test]
    fn markers_have_painted_pixels() {
        for m in [Marker::Player, Marker::Settlement, Marker::Tomb] {
            let rgba = marker(m);
            assert!(rgba.iter().any(|&b| b != 0), "marker is not empty");
            assert!(rgba.iter().any(|&b| b == 255), "marker has opaque pixels");
        }
    }
}
