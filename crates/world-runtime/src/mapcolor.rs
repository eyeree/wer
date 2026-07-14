//! False-color presentation helpers for the top-down debug map
//! (phase-1-plan.md section 10; phase-2-plan.md §11).
//!
//! These are the *shared* per-cell color ramps: the native CPU composer
//! (`platform-native`'s `viz.rs`), the WGSL atlas shader's reference values,
//! and the browser CPU map (phase-7-plan.md §4.1 milestone 2: "reuse the
//! existing CPU map composition path or a neutral equivalent") all paint from
//! this one table, so the map reads as the same world on every platform.
//! Everything here is pure `f32`/integer → RGB presentation — never a source
//! of identity (ADR 0003) — and touches no platform services, keeping the
//! module wasm-clean like the rest of this crate.

use world_core::{splitmix64, Biome, SEA_LEVEL};

/// Linear blend of two RGB colors.
#[must_use]
pub fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Missing-tile placeholder (dark checker so "not generated yet" is obvious).
#[must_use]
pub fn missing_color(cx: u16, cy: u16) -> [u8; 3] {
    if (cx / 4 + cy / 4) % 2 == 0 {
        [24, 24, 28]
    } else {
        [32, 32, 38]
    }
}

/// Elevation ramp: water depth below [`SEA_LEVEL`], green → rock → snow above.
#[must_use]
pub fn elevation_color(e: f32) -> [u8; 3] {
    if e < SEA_LEVEL {
        // Deep to shallow water.
        lerp_rgb(
            [8, 16, 64],
            [70, 130, 190],
            (1.0 + e / 600.0).clamp(0.0, 1.0),
        )
    } else {
        let t = (e / 900.0).clamp(0.0, 1.0);
        if t < 0.5 {
            lerp_rgb([70, 120, 60], [140, 120, 80], t * 2.0)
        } else {
            lerp_rgb([140, 120, 80], [245, 245, 245], (t - 0.5) * 2.0)
        }
    }
}

/// Air-temperature ramp (cold blue → hot red).
#[must_use]
pub fn temperature_color(t: f32) -> [u8; 3] {
    lerp_rgb([40, 60, 200], [220, 60, 40], (t + 15.0) / 50.0)
}

/// Surface-moisture ramp (dry tan → wet blue).
#[must_use]
pub fn moisture_color(m: f32) -> [u8; 3] {
    lerp_rgb([150, 110, 70], [40, 90, 200], m)
}

/// River-expression ramp over the stable drainage topology.
#[must_use]
pub fn river_color(r: f32) -> [u8; 3] {
    lerp_rgb([20, 20, 26], [80, 170, 255], r)
}

/// Surface-wetness ramp.
#[must_use]
pub fn wetness_color(w: f32) -> [u8; 3] {
    lerp_rgb([120, 100, 70], [30, 120, 160], w)
}

/// Soil: fertility hue, depth brightness.
#[must_use]
pub fn soil_color(depth: f32, fertility: f32) -> [u8; 3] {
    let hue = lerp_rgb([190, 170, 130], [80, 60, 30], fertility);
    let brightness = 0.35 + 0.65 * depth;
    [
        (f32::from(hue[0]) * brightness) as u8,
        (f32::from(hue[1]) * brightness) as u8,
        (f32::from(hue[2]) * brightness) as u8,
    ]
}

/// Aggregate vegetation density ramp.
#[must_use]
pub fn vegetation_color(v: f32) -> [u8; 3] {
    lerp_rgb([190, 175, 130], [20, 110, 40], v)
}

/// Herbivore pressure (aggregate ecology, L8).
#[must_use]
pub fn herbivore_color(h: f32) -> [u8; 3] {
    // Pressures are ecologically small (~10% steps down the pyramid); amplify
    // for legibility so a debug map still reads.
    lerp_rgb([20, 24, 20], [210, 200, 60], (h * 8.0).clamp(0.0, 1.0))
}

/// Predator pressure (aggregate ecology, L8).
#[must_use]
pub fn predator_color(p: f32) -> [u8; 3] {
    lerp_rgb([22, 18, 20], [220, 70, 60], (p * 40.0).clamp(0.0, 1.0))
}

/// Species-diversity ramp (aggregate ecology, L8).
#[must_use]
pub fn diversity_color(d: f32) -> [u8; 3] {
    lerp_rgb([30, 20, 45], [90, 220, 200], d)
}

/// A categorical colour for a species id: hash to a vivid, well-separated hue.
#[must_use]
pub fn species_color(species_id: u64) -> [u8; 3] {
    let h = splitmix64(species_id);
    // Bias toward saturated, mid-bright colours so distinct species read apart.
    [
        96 + (h & 0x7F) as u8,
        96 + ((h >> 20) & 0x7F) as u8,
        96 + ((h >> 40) & 0x7F) as u8,
    ]
}

/// Distinct tints per lithology class (geology channel), shaded by hardness.
const LITHOLOGY_TINTS: [[u8; 3]; 8] = [
    [188, 143, 122],
    [140, 150, 170],
    [172, 165, 120],
    [120, 160, 140],
    [180, 130, 160],
    [150, 140, 100],
    [110, 140, 175],
    [165, 120, 100],
];

/// Rock: lithology tint shaded by hardness (stable under drift).
#[must_use]
pub fn geology_color(world_x: f64, world_y: f64, hardness: f32) -> [u8; 3] {
    let tint = LITHOLOGY_TINTS[world_core::lithology_id(world_x, world_y) as usize];
    let shade = 0.45 + 0.55 * hardness;
    [
        (f32::from(tint[0]) * shade) as u8,
        (f32::from(tint[1]) * shade) as u8,
        (f32::from(tint[2]) * shade) as u8,
    ]
}

/// The categorical biome palette (phase-2-plan.md §11).
#[must_use]
pub const fn biome_color(biome: Biome) -> [u8; 3] {
    match biome {
        Biome::Ocean => [24, 44, 110],
        Biome::River => [58, 120, 216],
        Biome::Wetland => [70, 120, 110],
        Biome::Desert => [225, 200, 140],
        Biome::Grassland => [150, 180, 90],
        Biome::Shrubland => [170, 160, 100],
        Biome::TemperateForest => [45, 120, 55],
        Biome::Rainforest => [15, 95, 45],
        Biome::Taiga => [60, 100, 80],
        Biome::Tundra => [160, 160, 140],
        Biome::Bare => [130, 125, 120],
        Biome::Ice => [235, 240, 248],
    }
}

/// Composite: real biomes over water depth, with river/wetness expression
/// blended in so drift visibly breathes without moving the network.
#[must_use]
pub fn composite_color(e: f32, biome: Biome, river: f32, wetness: f32) -> [u8; 3] {
    if e < SEA_LEVEL {
        return elevation_color(e);
    }
    let mut rgb = biome_color(biome);
    // Rivers draw as blue veins; wetness darkens the ground toward marsh.
    rgb = lerp_rgb(rgb, [58, 120, 216], river * 0.8);
    rgb = lerp_rgb(rgb, [35, 60, 70], wetness * 0.25);
    // High rock fades in above the vegetation line.
    lerp_rgb(rgb, [130, 125, 120], ((e - 500.0) / 400.0).clamp(0.0, 1.0))
}

/// The POV sea-floor albedo: wet sand at the waterline picking up a cyan
/// water-absorption cast within the first ~25 units of depth, then falling
/// off into dark teal by ~140 — the depth tint real shallows have, baked
/// into the floor so it shades with the terrain while the surface stays a
/// subtle film. The 2D map keeps [`elevation_color`]'s blue *depth* ramp (a
/// map legend, not a material); painting the 3D sea floor with that blue
/// made the ocean read as opaque blue terrain instead of water over sand,
/// so the POV mesher diverges here deliberately. Presentation-only, like
/// every color in this module.
#[must_use]
pub fn pov_sediment_color(e: f32) -> [u8; 3] {
    let depth = -e;
    if depth <= 25.0 {
        lerp_rgb(
            [168, 150, 118],
            [96, 124, 118],
            (depth / 25.0).clamp(0.0, 1.0),
        )
    } else {
        lerp_rgb(
            [96, 124, 118],
            [12, 34, 44],
            ((depth - 25.0) / 115.0).clamp(0.0, 1.0),
        )
    }
}

/// The full per-cell Composite color — [`composite_color`] plus the
/// dominant-species land tint — shared by the 2D map's `paint_region`, the
/// 3D mesher's per-vertex material (3d-phase-1-plan.md §6.4), and the browser
/// CPU map (phase-7-plan.md §4.1), so every presentation reads as the same
/// world. `dominant` is the resolved species id
/// ([`crate::RegionMap::dominant_species_id`]); `None` paints untinted,
/// exactly as the 2D path does before ecology settles.
#[must_use]
pub fn composite_cell_color(
    e: f32,
    biome: Biome,
    river: f32,
    wetness: f32,
    dominant: Option<u64>,
) -> [u8; 3] {
    let rgb = composite_color(e, biome, river, wetness);
    // Tint land by dominant-species colour so ecosystem zonation reads at a
    // glance (phase-3-plan.md §11); open water keeps the depth ramp.
    match dominant {
        Some(id) if e >= SEA_LEVEL => lerp_rgb(rgb, species_color(id), 0.18),
        _ => rgb,
    }
}
