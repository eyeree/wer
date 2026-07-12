# Architecture Decision Records

This directory holds Architecture Decision Records (ADRs): short documents that
capture a significant architectural decision, its context, and its consequences.

ADRs are numbered sequentially and never deleted. When a decision is revised, add
a new ADR that supersedes the old one (and note it in the old record) rather than
editing history.

Format: [Michael Nygard's template](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

| # | Title | Status |
|---|-------|--------|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-workspace-crate-boundaries.md) | Workspace and crate boundaries | Accepted |
| [0003](0003-deterministic-integer-hashing.md) | Deterministic integer hashing for identities | Accepted |
| [0004](0004-terrain-noise-and-weak-possibility-coupling.md) | Terrain noise: hashed-gradient fBm with weak possibility coupling | Accepted |
| [0005](0005-drift-dirties-only-possibility-dependent-layers.md) | Possibility drift dirties only possibility-dependent layers | Superseded by 0007 |
| [0006](0006-travel-fueled-convergence.md) | Convergence is fueled by player travel, not wall-clock time | Accepted |
| [0007](0007-declared-layer-dependencies.md) | Declared layer dependencies supersede the static drift mask | Accepted |
| [0008](0008-tiles-are-functions-of-their-dependency-hash.md) | Tiles are functions of their dependency hash | Accepted |
| [0009](0009-drainage-topology-from-quantized-elevation.md) | Drainage topology from quantized elevation at macro level | Accepted |
| [0010](0010-species-identity-presentation-grade-until-atlas.md) | Species identity is presentation-grade until the atlas needs otherwise | Accepted |
| [0011](0011-anchors-capture-trait-targets-combine-order-independently.md) | Anchors capture trait targets and combine order-independently | Accepted |
| [0012](0012-resonance-gates-transition.md) | Resonance gates transition; it multiplies the travel-fueled rate | Accepted |
| [0013](0013-shareable-records-quantized-at-persistence-boundary.md) | Shareable records are quantized at the persistence boundary | Accepted |
| [0014](0014-vault-stores-deviations-with-crdt-merge.md) | The vault stores deviations, keyed by content-derived ids, with CRDT merge laws | Accepted |
| [0015](0015-routes-attract-as-derived-anchors.md) | Routes attract as derived anchors; attraction is soft and saturating | Accepted |
| [0016](0016-simd-kernels-bit-identical-to-scalar-twins.md) | SIMD kernels are lane-wise bit-identical to their scalar twins | Accepted |
| [0017](0017-gpu-compute-is-derived-presentation.md) | GPU compute is derived presentation; authoritative state never reads it back | Accepted |
| [0018](0018-settled-state-is-schedule-independent.md) | Settled world state is schedule-independent; budgets/tiers scale pacing, never identity | Accepted |
