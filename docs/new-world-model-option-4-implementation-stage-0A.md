# World Loom Stage 0A implementation plan

## Status, scope, and decision boundary

This plan implements the Stage 0A mathematical-kernel gate from
[`new-world-model-option-4.md`](new-world-model-option-4.md). It is a clean,
topology-independent experiment that coexists with the current prototype. It
does not change `WORLD_ALGORITHM_VERSION`, any prototype layer revision, world
generation, persistence, viewer behavior, or accepted ADR history.

The deliverable is deliberately smaller than a complete World Loom. It answers
the Stage 0A kill-gate question: can bounded typed packets and a deterministic
fixed-point transport/minimizing-movement step produce canonical, certified
answers at interactive cost? Planetary realization, general grammar compilation,
full Yearning semantics, Resonance, Transition Plans, and visualization begin no
earlier than Stage 0B/1.

Stage 0A is successful only when the implementation and its frozen corpus meet
the correctness, native/wasm parity, resolution-rate, and latency gates below.
Randomized evidence is not treated as proof for unbounded inputs.

## Deliverables

1. `loom-core`, a platform-neutral crate containing:
   - checked Q24 fraction and length arithmetic;
   - typed 64-atom material/trait measures;
   - the bounded two-law grammar and optional trait rewrite;
   - canonical `StatePacket` normalization, encoding, decoding, and SHA-256 root;
   - canonical normalized intent aggregation;
   - explicit validation errors and hard Stage 0A ceilings.
2. `loom-transport`, a platform-neutral crate containing:
   - a sparse/separable fixed-point unbalanced transport block;
   - deterministic candidate enumeration and lexicographic mode ordering;
   - one path-constrained minimizing-movement step;
   - `Complete`/`Unresolved` outcomes and replayable segment certificates;
   - pure selection and prefix advancement with conserved unused credit.
3. `wer-loom`, a CI-friendly native sign-off harness containing:
   - exhaustive small-state checks;
   - 10,000 deterministic randomized permutation/schedule cases;
   - a frozen representative and adversarial corpus;
   - latency and resolution ledgers with nonzero exit on a failed gate.
4. Native unit/integration tests and an actual Node wasm parity test in
   `platform-web` for frozen canonical bytes, roots, mode ids, endpoints, and
   certificate bytes.

## Contract frozen for the experiment

### Numeric formats and ceilings

- Fractions use unsigned Q24 in `0..=2^24`; checked operations reject overflow.
- Directed path length uses unsigned integer micro-length units. Length is
  additive, so no square-root or floating-point operation enters identity.
- A measure has one semantic kind (`Material` or `Trait`), one level (Stage 0A
  permits levels `0..4`), no more than 64 atoms, and an exact declared integer
  total.
- A packet has at most 4,096 explicit entries and canonical encoded size no
  greater than 64 KiB. Stage 0A emits only two measures and one optional rewrite,
  but the public validator enforces the architectural caps.
- A probe considers no more than eight modes. The ratified Stage 0A chain metric
  uses a finite exact integer dual program rather than convergence-qualified
  scaling. Exceeding a request or arithmetic bound fails closed.

### Two-law grammar

- Law 0 is a conserved material measure. Transport may move material between
  atoms but may not create or destroy its exact total.
- Law 1 is a trait-capacity measure. It may move mass and may create/destroy it
  within the declared Q24 capacity using a fixed birth/death penalty.
- The only Stage 0A rewrite activates the trait law from the canonical zero-mass
  boundary. It has a fixed positive directed length and normalizes away whenever
  trait mass is zero. Consequently rewrite history is excluded from state.
- Atom ground cost is the manifest-frozen absolute atom-index distance. Costs,
  penalties, solver revision, and tie rules are compile-time constants in this
  experiment and are committed by packet/mode/certificate version fields.

### Canonical packet normalization

Normalization performs, in this order:

1. validate packet/format/program versions and measure kinds;
2. sort entries by `(kind, level, atom)`;
3. combine duplicate atoms using checked addition;
4. remove zero entries;
5. verify atom, level, entry, total, and byte ceilings;
6. require material total to equal its declared inventory;
7. require trait total not to exceed its declared capacity;
8. derive rewrite activation from nonzero trait mass;
9. encode fields in one explicit big-endian format and hash those bytes.

The decoder rejects noncanonical bytes: decoding and renormalizing must reproduce
the input byte-for-byte. This gives one accepted encoding for each normalized
representable Stage 0A state.

### Intent and solver semantics

An input intent is a multiset of weighted atom deltas with stable ids. Canonical
aggregation sorts by id and content, combines identical terms with checked
integer arithmetic, and rejects conflicting reuse of an id. Input order and task
schedule therefore cannot affect the normalized request digest.

For each enabled path signature, the solver builds a target by applying the
normalized signed deltas, clamps only where the typed law permits it, and then
performs one bounded deterministic projection. Material conservation is restored
canonically; trait deficit/excess may use the fixed birth/death edge. An exact
finite dynamic program solves the one-dimensional chain-transport dual for every
law/level block. Every accepted endpoint is normalized through `loom-core`.

The Stage 0A certificate records source/destination roots, request digest,
normalized path signature, quantized directed length and limit, exact inventory
residuals, lower/upper objective bounds, and solver revision. The checker does
not trust the producer: it recomputes roots, request digest, endpoint validity,
path length, residuals, and mode id from supplied canonical inputs.

`Complete` means every enumerated mode has a feasible endpoint, its objective
interval is closed by the fixed Stage 0A checker, and the complete top-three
prefix is known. Otherwise the result is `Unresolved` with a typed reason; no
platform-dependent best effort is exposed as canonical.

## Work sequence

### A. Workspace and crate boundaries

- Add `loom-core` and `loom-transport` as workspace members.
- Make `loom-transport` depend only on `loom-core`.
- Add both to the repository's wasm check surface.
- Keep clocks, threads, files, sockets, random devices, and graphics out of both
  crates. The native harness owns timing and corpus reporting.

### B. `loom-core`

- Implement checked Q24 newtypes and signed intent deltas.
- Implement typed atoms, measures, normalized intent, grammar/program version,
  packet builder, canonical codec, and root.
- Make construction return `Result`; do not offer a public unchecked packet.
- Add unit tests for all bounds, duplicate folding, zero removal, exact totals,
  rewrite zero-boundary behavior, round trips, noncanonical rejection, and
  permutation-invariant intent bytes.

### C. `loom-transport`

- Implement the fixed-cost sparse transport primitive and exact typed balance
  checks.
- Enumerate the no-rewrite and optional-rewrite signatures in stable order;
  canonicalize and deduplicate by signature.
- Implement fixed-round target relaxation, objective bounds, mode ids, complete
  lexicographic ordering, and at most two alternatives.
- Implement certificate encode/check, explicit plan selection, and additive
  prefix advancement. Zero supplied credit must return the source packet.
- Add tests for conserved versus unbalanced laws, alternate-id stability,
  schedule/permutation independence, certificate tampering, frame-cadence
  independence, and unresolved caps.

### D. Harness and corpora

- Freeze ordinary probes spanning no-op, single-law, combined-law, rewrite, and
  compromise requests. Freeze adversarial probes at capacity, quantization,
  conflicting-delta, mode-tie, and length boundaries.
- Exhaustively enumerate small two-/three-atom distributions and intent orders.
- Generate 10,000 cases with a repository-local deterministic integer generator;
  shuffle both request insertion and simulated task completion order.
- For every case compare canonical request bytes, mode ordering and ids, endpoint
  bytes/roots, and certificate bytes against the canonical schedule.
- Record ordinary complete rate, adversarial complete/unresolved counts, and
  native latency distributions. Gate ordinary completion at 99%, packet
  normalization below 1 ms per frozen packet, and Egress below 4 ms per ordinary
  probe on the existing release-harness convention. Wasm timing is measured by
  its host test and gated below 10 ms per probe after warmup.

### E. Native/wasm parity

- Define a small frozen parity fixture with literal expected canonical bytes,
  roots, endpoint bytes, mode ids, and certificate bytes.
- Run it in native tests and in `wasm-pack test --node crates/platform-web`.
- Compare integers/bytes only; wall-clock values never enter fixtures.

### F. Verification and completion

Run the repository CI surface after implementation:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p viewer-host -p platform-web \
  -p loom-core -p loom-transport --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
cargo run --release --bin wer-loom
```

The stage is not declared passed if a required command is unavailable or if the
measured wasm/latency gate has not run. In that case the implementation may be
complete while the Stage 0A research gate remains explicitly unratified.

## Exit checklist

- [x] Canonical packet normalization is injective on the implemented fragment.
- [x] Types, units, totals, capacity, format, and complexity bounds fail closed.
- [x] Canonical results survive cold recomputation, input permutation, simulated
      task schedule, and cancellation. Cache warmness remains out of scope until
      a Loom cache exists.
- [x] Native and Node wasm frozen vectors are byte-identical for the ratified
      solver and certificate format.
- [x] One canonical default and stable, semantically distinct control modes are
      returned.
- [x] Every complete mode carries independently checked primal feasibility and
      dual optimality witnesses, not only same-solver replay.
- [x] Zero credit commits zero Egress; equal cumulative credit commits equal
      prefixes independent of frame cadence.
- [x] Exhaustive small cases and 10,000 randomized cases pass, with balanced
      small cases checked against an independent cumulative-flow oracle.
- [x] At least 99% of the preregistered ordinary corpus is complete within both
      native and wasm interaction targets.
- [x] Capacity, quantization, conflicting-id, inventory, witness-tamper,
      schedule/cancellation, and length adversaries report resolution by reason.

## Ratification correction and remediation

The first implementation demonstrated canonical packets, portable frozen
vectors, and ample latency headroom, but its forward/reverse repair routine was
not the transport/minimizing-movement solver described above. Repeating an
already-stable repair 24 times did not constitute a scaling solve; assigning the
same computed cost to certificate lower and upper bounds did not independently
prove optimality; reversing request order did not simulate task schedules; and
the two-case adversarial corpus was not representative. Consequently Stage 0A
was not ratified by that run.

Ratification now additionally requires:

1. an exact discrete one-dimensional transport oracle for the Stage 0A ground
   metric, with bounded creation/destruction for the trait law;
2. a dual potential witness checked by code that does not call the planner;
3. alternate modes that differ by normalized control magnitude or rewrite path,
   never merely by solver traversal order;
4. simulated job completion, cancellation, and cold recomputation schedules that
   all settle to identical canonical bytes (warm-cache checks begin with a Loom
   cache); and
5. preregistered ordinary and adversarial tables whose membership is fixed in
   source before the measured gate is run.
