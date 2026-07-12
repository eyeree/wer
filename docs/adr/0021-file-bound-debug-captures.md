# 21. File-bound debug captures may read GPU pixels; world state still never does

Date: 2026-07-12

## Status

Accepted

Amends [ADR 0017](0017-gpu-compute-is-derived-presentation.md) with one
narrow exception; everything else in ADR 0017 stands unchanged.

## Context

3D-1 renders the POV terrain entirely on the GPU. Unlike the 2D map — whose
CPU composer gives `wer --screenshot` a deterministic, GPU-free image path —
the POV view has no CPU rendering twin, so there was no way to inspect it
headlessly: no screenshots for bug reports, no scripted before/after captures
for debugging camera or lighting regressions. ADR 0017's structural rule
("the renderer exposes no readback API") was written to keep GPU-computed
values out of authoritative state; taken literally it also blocks a debug
screenshot, which feeds nothing but a human's eyeballs.

## Decision

**A dedicated headless capture type (`renderer::pov::PovCapture`) may copy
rendered pixels back to the CPU, for image-file output only.** The exception
is deliberately narrow:

- The live `Renderer` — the type the interactive shell holds — still exposes
  **no readback API**. `PovCapture` is a separate headless construction
  (offscreen texture, no surface) used only by debug tooling
  (`wer --pov-script`).
- Captured bytes go to image files for humans. They are never hashed into
  identities, persisted into the vault, compared by CI gates, or consumed by
  gameplay, steering, or generation code. The shell writes them to disk and
  drops them.
- GPU output remains non-portable bits (ADR 0017's context): a captured
  image is a *debug artifact*, valid for the machine that rendered it, and
  must never become a golden fixture. Image-based determinism checks stay on
  the CPU composer path.

## Consequences

- POV rendering is inspectable and scriptable headlessly (`--pov-script`
  drives camera moves, simulated mouse look, and snapshots), which is how
  camera/lighting/meshing bugs get reproduced and fixed.
- The determinism story is untouched: nothing a GPU computes can reach
  hashed, shared, or persisted state; the structural guard on the live
  renderer survives.
- Any future non-debug readback (e.g. GPU erosion feeding generation) still
  requires its own successor ADR proving a portable, versioned contract —
  this exception does not open that door.
