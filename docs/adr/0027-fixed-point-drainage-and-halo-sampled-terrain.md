# 0027. Fixed-point drainage and halo-sampled Terrain boundaries

Date: 2026-07-12

## Status

Accepted

## Context

ADR 0009 made flow direction and accumulation integral after elevation was
rounded to centimeters, but the elevation feeding that rounding still passed
through floating-point possibility sampling, Perlin interpolation, fBm, and
scaling. A float threshold could therefore change permanent topology. Ordinary
Terrain also applied one realized Planetary/Geology vector to a whole region,
and Hydrology and Soils reconstructed slope with one-sided tile-edge
differences. Neighboring histories could create both height and derivative
seams.

ADRs 0003 and 0008 require stable integer identity and complete cache keys.
ADR 0016 requires same-math optimized presentation kernels. ADR 0017 keeps GPU
map composition derived and CPU state authoritative. ADR 0019 requires current
provenance validation at integration. ADR 0023 retains authoritative regional
history when disposable fields are parked.

## Decision

Drainage routing elevation is an isolated integer-only evaluator. Coordinates,
gradients, fade weights, interpolation, relief, and Planetary/Geology scaling
use signed Q30 values. Products, weighted sums, and centimeter conversion use
`i128`. Division rounds to nearest with ties away from zero for either sign.
Possibility components come directly from the same SplitMix control-point
stream as the ordinary field: its high 24 bits are bilinearly combined as an
exact rational, floored to 4096 buckets, and reconstructed as exact Q30 bucket
centers. Five noise octaves retain the existing hashed gradients, offsets, and
16:8:4:2:1 spectrum. The scalar and complete macro paths call this one
evaluator. Drainage dependency keys fold the raw stored field spacing in
addition to both layer revisions and macro coordinate.

This supersedes only ADR 0009's float-derived routing-elevation decision. Its
level-4 window, apron, eight-neighbor descent, 10/7 distance weights,
coordinate tie break, local minima, accumulation order, and expression model
remain in force. Apron-truncated accumulation remains an acknowledged
approximation.

Ordinary Terrain snapshots the absolute 3 by 3 level-0 neighborhood of realized
Planetary/Geology buckets. Resident authority supplies `current` even when its
fields are parked. A missing coordinate uses the anchor-free base
`PossibilityField` sample, projected and requantized under the existing base
rules. Terrain keys fold all 18 buckets in row-major coordinate order,
Planetary before Geology. Loading identical authoritative buckets is therefore
content-inert.

Every core and one-cell ghost position samples those region-center buckets by
bilinear interpolation. The interpolation cell and world coordinate are
constructed from absolute integer cell coordinates, so overlapping core and
ghost evaluation—including negative coordinates—uses identical fetches and
operations. Terrain retains the SIMD float fBm row kernel, scales every lane by
its halo sample, and rolls three `n+2` elevation rows. It atomically emits both
Elevation and a new Slope channel. Slope is a centered difference everywhere;
Hydrology and Soils consume that stored Terrain output rather than reconstruct
it locally.

An authority P/G bucket change notifies every field-active Terrain consumer in
the source's 3 by 3 closure. Convergence, preserve snaps, authority insertion,
radius removal, session restoration, and field-recipe changes use this one
path. Parked authority remains a source and does not itself dispatch. Expected
keys and ADR 0019 integration checks remain authoritative, so conservative
notifications are safe and stale neighbor jobs cannot publish.

Drainage routing elevation is identity-grade integer state. Terrain Elevation
and Slope remain presentation-grade floats. The GPU atlas retains its 13
presented float channels and explicitly omits CPU-only Slope; it still has no
readback path.

`WORLD_ALGORITHM_VERSION` remains 2. Terrain and Drainage independently move
their `algorithm_revision` from 0 to 1; all other layer revisions remain 0.
Only fixtures downstream of those layer keys/outputs are re-blessed. CI pins
`wasm-bindgen-test` 0.3.76 and `wasm-pack` 0.13.1 and executes every parity
probe in Node, including a broad signed fixed-topology fold.

## Consequences

Permanent routing no longer contains a floating-point decision threshold, and
the field recipe cannot collide in its macro cache key. Ordinary neighboring
histories blend continuously between absolute region-center samples and share
one centered slope stencil. Terrain keys and lifecycle invalidation are larger
because a region now depends on eight neighbors as well as itself.

A complete 32 by 32 field now has 14 `f32` channels plus its `u8` and `u16`
tiles: 60,416 logical payload bytes. Terrain jobs need two output buffers and
three rolling scratch rows. Fixed scalar routing gives up the former float SIMD
macro fill; declared costs and performance records must reflect measurement.

This decision does not make accumulation window-independent, turn logical
payload limits into allocator/RSS ceilings, or make GPU refinement bit-equal to
CPU presentation.
