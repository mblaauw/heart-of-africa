# Heart of Africa — Style Bible (v1)

One page. Every asset — procedural or painted — must pass this test: it could
have been a lithograph in an 1890s expedition journal.

## Palette (the biome map colors)

| Biome | Hex | Mood |
|---|---|---|
| water | `#3A6EA5` | Nile blue, calm, opaque |
| desert | `#D9B36C` | sun-bleached sand |
| floodplain | `#8FAE5C` | irrigated green |
| savanna | `#C4B454` | dry grass gold |
| mountain | `#8C7A6B` | weathered stone |
| oasis | `#5FBF8F` | palm-spring green |

Markers: player `#FFD64D` (gold), settlements `#FFFFFF` (bone white),
tomb `#D93F33` (red rock).

## Rules

1. **Muted, painterly, low contrast.** Nothing saturated; texture is subtle
   (±8% brightness noise), never busy. The desert should feel large and
   quiet, not patterned.
2. **Period illustration, not modern pixel art.** Flat color fields with
   soft variation; no outlines except on markers.
3. **Content-ID addressing** (DESIGN.md §5): every asset resolves through
   `assets/assets.toml`; a missing asset falls back to the placeholder
   (tinted shape + label) and the game still runs.
4. **Procedural art is deterministic**: `hoatool gen-art` is seeded, so the
   same command produces byte-identical files. Regenerating is safe at any
   time; the committed PNGs are the canonical output.
5. **Tiles are 32×32, one atlas row per biome** (alphabetical order by
   biome id, matching the loader), two seeded variants per biome. Tiles
   only ever differ by a seeded value-noise pattern — never by content.

## Content voice (for text, not art — but same test)

Factual, respectful, period-true. The Scramble for Africa is ambient
pressure, never the player's job to resolve; brutality is stated, never
dramatized (DESIGN.md §3.2). NPC speech is plain and direct.
