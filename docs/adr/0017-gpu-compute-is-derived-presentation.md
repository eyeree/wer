# 17. GPU compute is derived presentation; authoritative state never reads it back

Date: 2026-07-11

## Status

Accepted

Builds on [ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md);
sibling of [ADR 0016](0016-simd-kernels-bit-identical-to-scalar-twins.md) and
[ADR 0018](0018-settled-state-is-schedule-independent.md).

## Context

Phase 6 moves the debug map from CPU pixel composition + full-texture
re-upload to GPU composition from a region-tile atlas, and adds refinement
octaves — WGSL continuing the terrain gradient-noise spectrum above
`FIELD_RES`, per screen pixel. Section 17 of the implementation plan sketches
a dual-resolution model (CPU authoritative low-res, GPU derived high-res) and
section 6.1 rules that authoritative state must not depend *exclusively* on
synchronous GPU readback. GPU float behavior varies by vendor, driver, and
backend; anything a GPU computes is inherently non-portable bits.

## Decision

**No value computed on the GPU is read back into authoritative state, hashed,
persisted, or consumed by gameplay, steering, or persistence code — not
"not exclusively", but not at all,** for everything Phase 6 builds and every
future GPU workload until a successor ADR carves a proven-portable exception.

Enforced structurally, not just by review:

- The renderer exposes **no readback API**. `render_map_gpu` accepts uploads
  and draws to the surface; there is no method that returns GPU-computed
  data to the caller. A violation is unwritable from the shell without
  changing the renderer's public surface, which review guards.
- `world-core` and `world-runtime` contain no GPU types and no renderer
  dependency (the crate boundary of ADR 0002 already forbids the reverse
  edge).
- Everything uploaded is a copy of CPU-authoritative tiles keyed by their
  dependency hashes (delta uploads are exact by ADR 0008: same key ⇒ same
  bytes).

Refinement is constrained to **zero-mean detail around the authoritative
sample** (gradient noise is zero-mean by construction), so CPU and GPU
presentations agree at tile resolution — checked visually via the A/B
toggle, never by a hash, because it is presentation. The CPU composer
remains the headless/screenshot/test path and the correctness reference;
CI renders nothing on a GPU, and image-based checks stay deterministic by
staying CPU.

## Consequences

- GPU work can be arbitrarily platform-varied (drivers, precision, backend)
  without any determinism risk: no output of it can reach state that is
  hashed, shared, or persisted.
- The browser renderer (Phase 7) inherits the same WGSL and the same
  one-way discipline unchanged.
- Future GPU candidates (ecology distribution, erosion, distance fields —
  section 17) must either stay derived-only under this ADR or wait for a
  successor that proves a portable, versioned readback contract.
- The pinned-violation detector and every harness keep reading world state,
  not pixels — unaffected by how the map is drawn.
