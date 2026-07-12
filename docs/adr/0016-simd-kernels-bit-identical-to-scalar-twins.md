# 16. SIMD kernels are lane-wise bit-identical to their scalar twins

Date: 2026-07-11

## Status

Accepted

Builds on [ADR 0003](0003-deterministic-integer-hashing.md) and
[ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md);
sibling of [ADR 0018](0018-settled-state-is-schedule-independent.md).

## Context

Phase 6 vectorizes the hot generation kernels (terrain fBm, climate, soils,
hydrology, vegetation — `world-core/src/simd.rs`). Vectorization is the
classic way determinism dies: FMA contraction changes rounding, reductions
reassociate, vector math libraries approximate transcendentals, and the
scalar and vector paths silently drift apart. The Phase 6 invariant is
absolute — no generated output changes for any input, zero golden fixtures
re-blessed — so "close enough" vectorization is not available.

The hot kernels happen to be per-cell independent maps: no cross-cell
reductions exist inside any of them, so lanes never need to interact and
exact IEEE lane arithmetic (`wide`, which never contracts) can reproduce the
scalar sequence exactly.

## Decision

1. **Vectorization is transcription.** A SIMD kernel is a lane-parallel
   transcription of its scalar kernel: the same operations, in the same
   order, per cell. No FMA (`mul_add` is banned in kernels), no
   reassociation, no fast-math, no cross-lane operations in `f32` paths.
   Branches become compare+blend transcriptions of the scalar branch
   semantics; transcendentals (`ln` in the river-width curve) run scalar per
   lane through the *same function* the scalar kernel calls.
2. **The scalar twin is permanent.** It is the spec, the tail path for rows
   not divisible by the lane width, the wasm fallback (`wide` compiles to
   scalar without `simd128`, keeping the browser build clean), and the
   differential oracle. The fast path is a twin, never a fork.
3. **Bit-equality is a CI gate.** `world-core/tests/simd_differential.rs`
   runs seeded randomized inputs plus edge sweeps (branch boundaries, signed
   zeros, denormal magnitudes) and asserts SIMD and scalar outputs are
   bit-equal on the platform under test. The golden fixtures are the second
   oracle: they pass unchanged by definition of the rule.
4. **Same-math caching is the only "algorithmic" speedup allowed.** Hoisting
   cell-invariant values (the fBm row's lattice gradients, the L8
   population table) is legal exactly when the cached values are the same
   bits the per-cell path would recompute. Anything that cannot meet
   bit-identity is an algorithm change and must go through a versioned
   `algorithm_revision` bump with fixture updates — in a later phase, not
   Phase 6.
5. **No runtime CPU dispatch.** `wide` picks the widest portable lane type
   at compile time (baseline x86-64, NEON, wasm `simd128` when enabled);
   runtime dispatch is deferred until a measured need.

Cross-*platform* `f32` remains per-platform presentation exactly as Phases
2–5 left it; the integer parity surfaces stay the cross-platform contract,
untouched.

## Consequences

- The entire Phase 2–5 fixture corpus doubles as the SIMD regression net;
  a re-bless appearing in a vectorization diff is a determinism bug by
  definition.
- Kernel changes now come in pairs: edit the scalar twin (the spec), then
  re-transcribe the SIMD path; the differential test fails loudly if the
  two diverge.
- Some vector transcriptions win nothing (climate, vegetation — already
  ~1–2 ns/cell scalar); they are kept only because the row interface is the
  portable seam and the tests pin them at zero risk. The paying kernels
  (fBm 2.6×, and the drainage fill riding it) carry the milestone —
  recorded in `docs/perf-baseline.md`.
