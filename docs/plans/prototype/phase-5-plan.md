# Phase 5 — Routes, Persistence, and Social Model: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 5 work begins
(it covers the ground of `persistence-plan.md` and `route-system-plan.md`, plus
the sparse-feature-store slice of the spatial-structures design in section 12.4
and the community-atlas slice of the social model). It expands the Phase 5
scope in section 20 into concrete interfaces, data layouts, algorithms, and
milestones, grounded in the landed Phase 4 stack
([`phase-4-plan.md`](phase-4-plan.md), ADRs 0011–0012) and the deliberately
pre-cut persistence seams: the unused `Storage` trait
(`world-runtime/src/storage.rs`), `AnchorSource` ("the seed of a
persistent/shareable discovery record in Phase 5"), the order-independent
`steer` (ADR 0011 made load order safe *specifically* for this phase), and the
ADR 0010 upgrade path ("quantize the classification inputs into portable bands
before the atlas needs otherwise").

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it assumes
them and calls out where Phase 5 stresses each.

---

## 1. Goals and non-goals

### 1.1 The question Phase 5 must answer

Phases 1–4 built a world that is generated, steered, and transformed entirely
in memory. Close the window and everything is gone: the anchors the player
captured, the worlds they shaped, the route they took to get there. The
`Storage` trait has sat unused since Phase 0 precisely so that the first real
persistence design could be made *after* the world model settled. Phase 4
finished settling it — steering is order-independent, captured anchors carry
their own trait targets, and the portable/presentation-grade split (ADR 0010)
tells us exactly which values may cross a platform boundary. Phase 5 asks:

> Can exploration leave **durable, shareable structure** — named discoveries,
> preserves, expedition routes, shared anchors, an atlas schema — **without
> storing generated world geometry**, and without loosening the continuity,
> determinism, or invalidation precision the earlier phases won? (section 20,
> Phase 5)

This is the direct validation of the Overview's **social vision**: players
leave persistent paths through possibility space, share anchors, publish
expeditions, and build a community atlas of species and landscapes. It is also
the first time any world state outlives a process, so it is where the
persistence-format and platform-divergence risks (sections 23.4–23.6) first
become real. The answer must be machine-checkable.

### 1.2 Success criterion (from section 20)

> Exploration creates durable, shareable structure without storing generated
> world geometry.

Decomposed into machine-checkable properties (asserted by the vault harness,
§12.3):

- **Durable:** a session saved mid-journey and reloaded into a fresh process
  settles to the *same* world — the two-run state hash (`replay.rs`) of the
  save→load→settle run equals the uninterrupted run's. Anchors, the player's
  position and bias, the resident window's realized possibility state,
  discoveries, preserves, and routes all survive.
- **Sparse:** the store contains **no tiles, no organisms, no geometry** — only
  deviations from deterministic generation (section 12.4). Store size is
  `O(player actions)`: a long scripted journey with `k` captures, namings,
  preserves, and route nodes stores bytes linear in `k` (plus a bounded session
  snapshot and a compact discovered-region set), independent of how much world
  was generated along the way.
- **Shareable:** a record exported from one run and imported into another
  reproduces the same effect — an imported shared anchor steers *identically*
  (the steering math is a cross-platform parity surface, AGENTS.md), an
  imported preserve realizes the same quantized possibility state and hence the
  same dependency hashes and integer-topology surfaces. Import order does not
  matter (ADR 0011), and merging two stores is commutative, associative, and
  idempotent — the "server-compatible persistence model" is a set of merge
  laws, not a server (§7.6).
- **Soft routes:** a recorded route creates a *soft attraction field*, not
  exact replay (section 13) — near a well-used corridor the possibility target
  bends toward the route's recorded signature, monotonically in usage count,
  and not at all beyond the corridor; a route can never force a region to an
  implausible or bit-exact remembered state.
- **Continuity & determinism preserved:** loading records never dirties a layer
  that the equivalent live action would not have dirtied (the invalidation
  ledger stays green); the continuity replay, ecology harness, and anchor
  harness all still pass; `WORLD_ALGORITHM_VERSION` stays at 2 (§9.1).

### 1.3 Goals

- **First real user of `Storage`** (section 18): a **vault** — a sparse,
  versioned record store layered on the key/value `Storage` trait — holding
  world/format version metadata, the session snapshot, discoveries, preserves,
  routes, and the discovered-region set. A `MemoryStorage` reference
  implementation (neutral, for tests and harnesses) and a native `FileStorage`
  (platform-native, atomic per-key writes).
- **A versioned record schema** (section 18): the workspace's first
  serialization — `serde` (declared in `[workspace.dependencies]` since Phase
  0, used by no crate yet) over a compact portable wire format, every record
  wrapped in an envelope carrying `RECORD_FORMAT_VERSION` and
  `WORLD_ALGORITHM_VERSION`, forward-migratable, safe for partial loading
  (every record is its own key), independent of native pointers, and
  browser-storage-compatible by construction (bytes in a k/v store).
- **Named discoveries** (Overview, Community Atlas): naming a capture persists
  a `DiscoveryRecord` — the discovery's *portable* identity (species seed,
  habitat-signature seed — the ADR 0010 integer core), its quantized captured
  trait target, quantized position, source, and player-given name/journal text.
  The record is the shareable form of a Phase 4 anchor.
- **Shared anchors** (Overview, Social Features): an anchor reconstructed from
  a `DiscoveryRecord` steers exactly like the original *everywhere* — the
  record stores quantized integers, `steer`/`project_plausible` are already
  cross-platform parity surfaces, so shared steering is portable end-to-end
  (§7.1). This cashes in ADR 0011's promise that order-independence made
  "persistence load order, shared anchors" safe.
- **Preserves** (Overview; section 18): a player marks a region window as
  preserved — its regions pin (`stability = 1`, no convergence, no steering)
  and the preserve persists only each region's **quantized possibility
  buckets** (a few dozen bytes per region). Reload — or import on another
  machine — re-derives the entire preserved landscape deterministically from
  the buckets: durable structure with zero stored geometry, the success
  criterion in miniature (§7.5).
- **Expedition routes and the possibility-space route graph** (section 13): a
  route recorder samples the journey at fixed travel intervals into
  `RouteNode`s — quantized position, quantized possibility signature,
  transition cost (from resonance), region stability, an order-independent
  anchor-set signature. Routes persist, accumulate usage counts, and project a
  **soft attraction field** implemented as derived weak anchors riding the
  existing order-independent steering algebra — no new steering math (§7.4).
  Route difficulty falls out of recorded node costs. An expedition is a named
  route plus its discovery references and journal text — publishable as a
  bundle.
- **Community atlas schema** (Overview, Community Atlas): the versioned
  `AtlasBundle` — discoveries, routes, preserves, journals — plus a `wer-atlas`
  tool to export, import, validate, and merge bundles. Schema and merge laws
  only; no server, no UI (§1.4).
- **Debug visibility**: save/load/preserve/route keys in the shell; discovered,
  route, and preserve overlays; vault stats in the panel; `wer-inspect
  --vault` / `--routes`; and a **vault harness** (`wer-vault`) that is the
  phase's machine-checkable sign-off — the analogue of the ledger, ecology, and
  anchor harnesses.

### 1.4 Non-goals (explicitly deferred)

- **Networking, servers, hosted worlds, live multiplayer.** "Server-compatible"
  means the record model *could* be served by a dumb content-addressed store —
  content-derived ids, order-independent merge, no local pointers — proven by
  file-based bundle exchange, not by sockets. The neutral crates still open no
  sockets; the overview's hosted shared worlds are a later product phase.
- **Browser storage backend** (Phase 7). Phase 5 delivers the format and the
  abstraction; `platform-web` gains parity exports for the *codec and shared
  steering math* (§12.5), not an IndexedDB/OPFS `Storage` implementation.
- **Photography, naming UI, journal UI.** Naming a discovery is a debug action
  that auto-generates a placeholder name; free-text entry, the camera, and
  museum/journal tools (Overview) are game-facing later phases. The
  *procedural* content — what a name durably refers to — is what Phase 5 builds.
- **Modified terrain, player-built structures, dead/replaced organism
  overrides** (section 12.4's remaining override kinds). No gameplay creates
  them yet; the vault's key namespace and envelope leave room, and adding a
  record kind is additive (§9.4).
- **Route pathfinding and guidance UI.** Phase 5 records routes and makes them
  attract; "searching for target ecosystems" over the route graph ships as an
  inspector query (§11), not an in-game navigator.
- **Growing the possibility vector.** Records quantize the existing 8-domain
  vector; per-category sub-traits still await a later fidelity phase. The atlas
  schema stores domains + the `TraitCategory` mask so records survive the
  vector's eventual growth (§9.4).
- **Persisting far-field world state.** Eviction semantics are unchanged: an
  evicted, unpreserved region re-derives from the field and its steering
  context, exactly as in Phases 1–4. Durability covers the resident window,
  player state, and the sparse records — not a transcript of every region ever
  visited.

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 5 coverage |
|---|---|
| `persistence-plan.md` | **Core of Phase 5** — vault, record schema, envelope/versioning, storage backends, session round-trip (§4–§7). |
| `route-system-plan.md` | **Core of Phase 5** — recorder, route graph, attraction field, usage/difficulty (§4.4, §7.3–7.4). |
| `region-streaming-plan.md` | Persistent overrides: preserve pinning and bucket restore in the load path (§5.2, §7.5). |
| `anchor-system-plan.md` | Shared-anchor slice: discovery records ⇄ anchors, quantized at the boundary (§7.1). |
| `possibility-space-plan.md` | Quantized possibility signatures as the portable record vocabulary (§4.2). |
| `determinism-and-versioning-plan.md` | The first on-disk format: `RECORD_FORMAT_VERSION`, migration, codec parity (§9). |

Generation is **untouched**: no layer changes, no new layer, no tile format
change. Phase 5 changes what *outlives* a frame and a process, and everything
it loads acts through the existing steering/stability machinery — a loaded
anchor is just an anchor, a route is derived anchors, a preserve is a pinned
target. There is deliberately no path by which a record reaches a generator
except through the possibility vector.

---

## 3. Architecture overview

Phase 5 adds no generation layer and no steering math. It adds a **record
boundary** (world-core), a **vault** that orchestrates records over `Storage`
(world-runtime), and **replay-side effects** that feed loaded records back into
the existing machinery:

```text
  Player actions (Phase 4 outputs)              Persistence boundary (§4, §7.1)
  ──────────────────────────────                ────────────────────────────────
   capture_at → Anchor            ─┐  name/keep  DiscoveryRecord { id, seeds,
   journey (pos, current, cost)    ├──────────▶  RouteRecord       quantized
   preserve command (window)      ─┘  quantize   PreserveRecord    integers +
   session (player, bias, anchors)   on write    SessionSnapshot   strings }
                                                        │
  Vault (world-runtime, budgeted, main thread)          ▼
  ────────────────────────────────────────────────────────────────
   dirty-record queue ─▶ envelope { format ver, world ver } ─▶ codec
        │                                                        │
        ▼                                                        ▼
   Storage trait (k/v bytes) ──── MemoryStorage (tests) / FileStorage (native)
                                                 │        [IndexedDB: Phase 7]
  Load / import (order-independent)              ▼
  ────────────────────────────────────────────────────────────────
   SessionSnapshot ─▶ player, bias, anchors (bit-exact, run-local tier)
   DiscoveryRecord ─▶ shared Anchor (dequantized target)  ─▶ steer (unchanged)
   RouteRecord     ─▶ derived weak anchors near corridor  ─▶ steer (unchanged)
   PreserveRecord  ─▶ possibility override: pinned buckets ─▶ load path/converge
   AtlasBundle     ─▶ merge by content id (commutative/assoc/idempotent)
```

Four commitments organize everything, each continuing an earlier commitment:

1. **Store only deviations; never geometry.** The vault persists intents and
   identities — quantized possibility states, seeds, positions, names — and the
   deterministic engine re-derives everything else (section 12.4, section 18:
   "generated base world data should be reconstructed deterministically"). This
   is the persistence-side twin of ADR 0008: tiles are functions of their
   dependency hash, so storing the hash's *inputs* (buckets) is storing the
   world.

2. **Portability is won at the record boundary.** Shareable records contain
   only integers and strings: `f32` values are quantized on write
   (`POSSIBILITY_QUANT` buckets for possibility vectors; coarse bands for
   strengths and costs). Live derivation stays presentation-grade exactly as
   ADR 0010/0011 left it — quantizing at the boundary is the named upgrade
   path, executed. A record therefore means the same thing on every platform,
   while the engine's runtime floats never cross a process boundary except in
   the run-local session tier (§6.3, ADR 0013).

3. **Sharing merges order-independently.** Record ids are content-derived from
   the record's immutable integer fields (splitmix64 fold, `hash.rs` style), so
   the same discovery yields the same id everywhere and merge is union-by-id
   with conflict-free immutable fields *by construction*; mutable fields (name,
   journal, usage) merge by deterministic max. Merge is commutative,
   associative, and idempotent — machine-checked (§7.6) — which is the entire
   "server-compatible" claim (ADR 0014).

4. **Loaded records act through existing machinery only.** A shared anchor
   enters the same `&[Anchor]` slice; route attraction is *derived anchors*
   reusing the order-independent `steer` (no second steering algebra to keep
   coherent, ADR 0015); a preserve is a stability pin plus a target override in
   the region load path. Nothing loaded can reach a generator except by moving
   a possibility vector — so projection, resonance gating, dep-hash staleness,
   and invalidation precision all apply to persisted influence for free.

---

## 4. Records (world-core)

### 4.1 The record module and envelope

New module `world-core/src/record.rs` — pure data + codec, wasm-clean,
`serde`-derived (the first crate to use the workspace `serde`; the wire format
is `postcard`, added to `[workspace.dependencies]` — `no_std`, compact, a
stable specified encoding; golden byte fixtures pin it regardless, §12.1):

```rust
/// Version of the on-disk/wire record encoding. Independent of
/// WORLD_ALGORITHM_VERSION: the format can evolve without touching world
/// identity, and vice versa. Bump on any schema change; add a migration (§9.4).
pub const RECORD_FORMAT_VERSION: u16 = 1;

/// Every persisted value is wrapped in this envelope. `world_version` records
/// which algorithm generation the record was created under; readers refuse
/// records from a *newer* format and migrate older ones forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub format_version: u16,
    pub world_version: u32,
    pub kind: RecordKind, // Session | Discovery | Route | Preserve | Seen | Meta
}
```

### 4.2 The portable possibility signature

Shareable records never contain an `f32` possibility value. The existing
quantization surface (`possibility.rs`: `quantized`, `from_quantized`,
`POSSIBILITY_QUANT = 4096`) becomes the record vocabulary:

```rust
/// A possibility vector quantized for persistence and sharing: one bucket per
/// domain. Dequantizing yields bucket centers — identical on every platform —
/// so any math downstream of a signature (steer, project, dep hashes) is
/// cross-platform by construction (ADR 0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PossibilitySignature {
    pub buckets: [u16; POSSIBILITY_DIMS],
}

impl PossibilitySignature {
    #[must_use] pub fn of(v: PossibilityVector) -> Self;          // quantize on write
    #[must_use] pub fn dequantize(self) -> PossibilityVector;     // bucket centers
    #[must_use] pub const fn seed(self) -> u64;                   // integer fold, hash.rs style
}
```

`seed()` folds `WORLD_ALGORITHM_VERSION` and the buckets with the portable mix
— the possibility-space analogue of `HabitatSignature::seed`, and the key by
which the route graph indexes possibility space (§7.4).

### 4.3 Discovery records (named discoveries, shared anchors)

```rust
/// A named, shareable discovery — the persistent form of a Phase 4 capture.
/// Identity fields are integers (portable, ADR 0010's core + this record's
/// quantized target); presentation strings ride along but never enter the id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryRecord {
    /// Content-derived id: mix-fold of the immutable integer fields below.
    /// Same discovery ⇒ same id everywhere ⇒ conflict-free merge (§7.6).
    pub id: u64,
    pub source: AnchorSource,           // Organism{species: u64} | Landform | …
    pub signature_seed: u64,            // portable habitat identity (ADR 0010)
    pub target: PossibilitySignature,   // quantized captured trait target
    pub mask: u8,                       // TraitCategory mask (Phase 4)
    pub kind: AnchorKind,
    pub strength_q: u16,                // strength quantized to the same grid
    pub falloff_q: u32,                 // falloff radius in integer world cells
    pub pos_q: (i64, i64),              // capture position, integer cells
    pub sequence: u64,                  // store-local monotonic; merge tiebreak
    pub name: String,                   // mutable; excluded from id
    pub journal: String,                // mutable; excluded from id
}

impl DiscoveryRecord {
    /// Quantize a live anchor into a record (the write boundary).
    #[must_use] pub fn from_anchor(a: &Anchor, sequence: u64, name: String) -> Self;
    /// Reconstruct a steering anchor (the read boundary). Pure and portable:
    /// dequantized integers in, so the anchor is identical on every platform.
    #[must_use] pub fn to_anchor(&self) -> Anchor;
}
```

### 4.4 Route records and the route graph

```rust
/// One sample along an expedition — the section 13 node shape, quantized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteNode {
    pub pos_q: (i64, i64),               // physical position, integer cells
    pub signature: PossibilitySignature, // possibility-space position
    pub cost_q: u8,                      // transition cost ≈ 1 − resonance, banded
    pub stability_q: u8,                 // region stability, banded
    pub anchor_sig: u64,                 // order-independent hash of active anchors
}

/// A persisted expedition: an ordered node path plus its social metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    pub id: u64,                 // content-derived from the node path
    pub nodes: Vec<RouteNode>,
    pub usage: u32,              // traversals; merge by max; attraction input
    pub discoveries: Vec<u64>,   // DiscoveryRecord ids made along the way
    pub sequence: u64,
    pub name: String,            // mutable; excluded from id
    pub journal: String,         // the expedition journal (section 18)
}
```

`world-core/src/route.rs` holds the pure route math: `route_difficulty(&[RouteNode])
-> f32` (aggregate of node costs), `anchor_set_signature(&[Anchor]) -> u64`
(XOR-fold of per-anchor quantized-field hashes — order-independent, consistent
with ADR 0011), and `attraction_anchors(...)` (§7.4). The **route graph** is a
derived, in-memory index — nodes keyed by `signature.seed()`, edges from path
adjacency — rebuilt from loaded `RouteRecord`s, never persisted itself
(section 13's graph is a view; the records are the truth).

### 4.5 Preserve, session, and seen records

```rust
/// A preserved window: pinned regions restored from quantized buckets alone.
/// No tiles, no organisms — deterministic generation reproduces the landscape
/// from `buckets` (ADR 0008), which is the success criterion in one struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreserveRecord {
    pub id: u64,                    // content-derived from coords + buckets
    pub regions: Vec<(RegionCoord, PossibilitySignature)>,
    pub sequence: u64,
    pub name: String,
    pub journal: String,
}

/// The run-local session tier (§6.3): bit-exact f32, never shared, never
/// merged, meaningful only on the platform that wrote it. This is the ONE
/// record kind allowed to carry raw float bits (as u32 bit patterns).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub player: (f64, f64),
    pub last_player: (f64, f64),
    pub bias_bits: [u32; POSSIBILITY_DIMS],
    pub transition_mode: bool,
    pub anchors: Vec<AnchorBits>,       // full-precision anchor state
    pub regions: Vec<RegionSnapshot>,   // resident window: coord, current bits,
    pub sequence: u64,                  //   stability bits, revision
}
```

The discovered-region set is stored as `SeenRecord`s — one per macro cell, a
sorted run-length list of visited region coords — so the atlas map and
"discovered regions" (section 18) cost a few bytes per region ever visited and
partial-load cleanly (only the macro cells near the player load).

---

## 5. Public interfaces

### 5.1 `world-core` additions

```text
world-core/src/
    record.rs   # NEW: Envelope, RECORD_FORMAT_VERSION, PossibilitySignature,
                #   DiscoveryRecord, RouteRecord/RouteNode, PreserveRecord,
                #   SessionSnapshot, SeenRecord, AtlasBundle, content ids,
                #   encode/decode + migration (§4, §7.2, §9.4)
    route.rs    # NEW: route graph index, attraction_anchors, route_difficulty,
                #   anchor_set_signature (§4.4, §7.4)
    possibility.rs # unchanged vector; PossibilitySignature lives in record.rs
```

`lib.rs` re-exports the record and route types. All new code is pure,
wasm-clean, allocation-bounded, in the `#[inline] #[must_use]` style. `serde`
and `postcard` become the first real dependencies of `world-core` beyond the
core language (both `no_std`; the wasm CI job enforces cleanliness).

### 5.2 `world-runtime` changes

```text
world-runtime/src/
    vault.rs    # NEW: Vault<S: Storage> — key namespace, dirty queue, budgeted
                #   flush, save/load/import/export orchestration, merge (§7.6–7.7)
    storage.rs  # Storage gains keys_with_prefix() (trait has zero implementors
                #   today, so this is free); NEW MemoryStorage reference impl
    route.rs    # NEW: RouteRecorder (samples the journey), usage tracking,
                #   active-route attraction assembly (§7.3–7.4)
    stream.rs   # possibility overrides: pinned/bucket-restored regions in the
                #   load path; retarget/converge skip pinned; seen-set observe
    budget.rs   # + max_persist_ops, + max_route_attraction_nodes (§8.3)
    region.rs   # unchanged (stability field already expresses pinning)
```

Key additions:

```rust
// vault.rs — persistence orchestrator. Owns no world state; reads and feeds
// the shell/RegionMap. Generic over Storage, so tests use MemoryStorage and
// the native shell uses FileStorage.
pub struct Vault<S: Storage> { /* store, sequence, dirty queue, loaded indexes */ }

impl<S: Storage> Vault<S> {
    pub fn open(store: S) -> Result<Self, StorageError>;       // meta/version check
    pub fn record_discovery(&mut self, a: &Anchor, name: String) -> u64;
    pub fn record_preserve(&mut self, regions: &[(RegionCoord, PossibilityVector)],
                           name: String) -> u64;
    pub fn record_route(&mut self, nodes: Vec<RouteNode>, discoveries: Vec<u64>,
                        name: String) -> u64;
    pub fn snapshot_session(&mut self, map: &RegionMap, shell: &SessionInputs);
    pub fn flush(&mut self, budget: &Budget) -> VaultStats;    // budgeted writes
    pub fn load_session(&self) -> Option<SessionSnapshot>;
    pub fn discoveries(&self) -> &BTreeMap<u64, DiscoveryRecord>; // + routes(),
    pub fn import(&mut self, bundle: &AtlasBundle) -> MergeStats; //   preserves()
    pub fn export(&self) -> AtlasBundle;
}

// stream.rs — persistent possibility overrides (section 6.4's field, realized).
// Set by the shell from loaded PreserveRecords; consulted by the region load
// path (current/target from buckets, stability = 1) and skipped by
// retarget/converge. Overrides are tiny and survive eviction.
pub fn set_override(&mut self, coord: RegionCoord, sig: PossibilitySignature);
pub fn clear_override(&mut self, coord: RegionCoord);
pub fn restore_region(&mut self, snap: &RegionSnapshot);  // session load path
```

`RegionMap::update`'s signature is **unchanged**. Route attraction arrives as
derived anchors appended to the existing `&[Anchor]` slice by the shell/harness
(§7.4), and preserves act through the override table — the map never learns
about `Storage`, records, or the vault, keeping the streaming core exactly as
testable as it is today.

### 5.3 `platform-native`, `tools`, `platform-web`, `renderer`

- **`platform-native`:** `FileStorage` (a directory; key → escaped filename;
  write-temp-then-rename atomicity; `keys_with_prefix` via directory listing).
  Shell keys: save session / load session; name-last-capture (auto-generated
  placeholder name); preserve the near window; route-recording toggle;
  route-attraction toggle. New overlays and panel lines (§11).
- **`tools`:** the **vault harness** (`wer-vault`, lib module + thin bin, the
  established pattern) driving the §12.3 scenarios — the Phase 5 sign-off. The
  **`wer-atlas`** tool: `export`, `import`, `check` (validate + merge-law
  self-test), `list` over stores and bundle files. `wer-inspect --vault X Y`
  and `--routes X Y` (§11).
- **`platform-web`:** two parity exports — `record_codec_sample()` (hash of the
  canonical encoded bytes of a fixed record set: byte-level format portability)
  and `shared_steer_sample()` (steer + project from a fixed `DiscoveryRecord`
  via `to_anchor()`: shared-anchor steering is portable end-to-end) — pinned to
  the native goldens. No browser storage backend (§1.4).
- **Renderer:** unchanged — still one composed debug texture.

---

## 6. Data layout

### 6.1 Key namespace

One record per key so partial loading is structural (section 18):

| Key | Value | Cardinality |
|---|---|---|
| `meta/store` | store header: format version, world version, sequence counter | 1 |
| `session/current` | `SessionSnapshot` | 1 (overwritten) |
| `disc/<id:016x>` | `DiscoveryRecord` | per named discovery |
| `route/<id:016x>` | `RouteRecord` | per expedition |
| `pres/<id:016x>` | `PreserveRecord` | per preserve |
| `seen/<macro:016x>` | `SeenRecord` (run-length region coords) | per visited macro cell |

Keys are ASCII, caller-namespaced exactly as `storage.rs`'s docs anticipated,
and map unchanged onto a file tree or a browser object store.

### 6.2 Record sizes (why "sparse" holds)

A `DiscoveryRecord` is ~100 bytes + name/journal; a `RouteNode` is ~40 bytes
(a 1,000-node expedition ≈ 40 KB); a `PreserveRecord` costs ~24 bytes *per
region* (coord + 8 buckets); a `SeenRecord` a few bytes per visited region; the
`SessionSnapshot` ~50 bytes per resident region — bounded by the streaming
window, not by travel. Nothing scales with tiles, organisms, or generated
detail. The harness asserts the store-size bound directly (§12.3).

### 6.3 The two persistence tiers

| Tier | Records | Float policy | Scope |
|---|---|---|---|
| **Session** (run-local) | `SessionSnapshot` | bit-exact `f32`/`f64` as bit patterns | same platform, same run-lineage; never merged, never shared; exists so save→load is *exact* (state-hash equal) |
| **Shareable** | discovery, route, preserve, seen, atlas bundles | **no floats** — quantized integers only | portable across platforms and stores; merged by content id |

This split is the load-bearing decision (ADR 0013): exactness where durability
demands it, portability where sharing demands it, and never a confusion of the
two. A live anchor and its discovery record coexist: the session tier keeps the
run's own anchors bit-exact, while the record is the quantized shareable shadow.

### 6.4 In-memory vault state

The vault holds loaded record indexes (`BTreeMap<u64, _>` — deterministic
iteration order, matching the map's conventions), a dirty-id queue, and the
monotonic sequence counter. The route graph index and per-frame derived
attraction anchors are rebuilt views, never persisted. The possibility-override
table in `RegionMap` is `BTreeMap<RegionCoord, PossibilitySignature>` — a few
dozen bytes per preserved region, retained across eviction (that is its job).

---

## 7. Algorithms

### 7.1 Quantize-on-write, content ids (the record boundary)

`DiscoveryRecord::from_anchor` quantizes the captured `f32` target through
`PossibilitySignature::of`, bands strength/falloff onto integer grids, and
quantizes the position to integer cells. The content id is a `mix`-fold (the
`hash.rs` primitive) over a domain-separating kind tag and the immutable
integer fields — source, signature seed, target buckets, mask, kind, strength
band, position — in a fixed documented field order (golden-fixtured; the fold
order is part of the format contract exactly as `feature_hash`'s is). Names,
journals, and sequence numbers are excluded: renaming never changes identity,
and the same capture yields the same id in any store.

`to_anchor` dequantizes to bucket centers. Consequences, both machine-checked:

- **Round-trip epsilon:** an anchor reconstructed from its own record steers
  within quantization epsilon (≤ half a bucket per domain) of the live anchor —
  asserted by the harness so quantization loss stays a non-event.
- **Cross-platform identity:** the *record* steers bit-identically everywhere,
  because its inputs are integers and `steer`/`project_plausible` are the
  already-parity-tested float-deterministic surfaces (`steer_sample`). This is
  the shared-anchor guarantee, and it needs no new math (§12.5).

### 7.2 Codec, envelope, migration

Encode = envelope + postcard body, little-endian, no pointers, no platform
words. Decode checks `format_version`: newer than ours → `StorageError`
(refuse, don't corrupt); older → run the per-version upgrade chain
(`migrate_v1_to_v2`, …) — pure functions kept forever, tested against
golden-encoded old-version bytes so forward migration never rots (§12.1).
`world_version` mismatches don't block loading (records are quantized intents,
not generated output), but the mismatch is surfaced in `wer-atlas check` and
the panel, because a bucket means a different realized world under a different
algorithm version — an honest label, not a lie of equivalence.

### 7.3 Route recording

`RouteRecorder::observe(map, player, travel, anchors)` runs once per frame
while recording: it accumulates travel and emits a `RouteNode` every
`ROUTE_SAMPLE_SPACING` units — position quantized, the covering region's
*target* signature quantized (the possibility-space coordinate), `cost_q` from
the frame's resonance strength (low resonance = hard transition = costly node —
"route difficulty" falls out of the world model, not a designer knob),
`stability_q`, and `anchor_set_signature(anchors)`. Stopping the recorder (or
crossing `MAX_ROUTE_NODES`) closes the path into a `RouteRecord`; discoveries
named during recording attach their ids. Deterministic per run by construction:
every input is frame state the replay already reproduces.

### 7.4 Route attraction (derived anchors — no new steering algebra)

Each frame, for the active (followed) routes, the shell/harness assembles
derived anchors from the route nodes within attraction range of the player,
capped at `max_route_attraction_nodes` nearest-first:

```text
for node near player:
    Anchor {
        world_pos: node position,
        target:    node.signature.dequantize(),
        mask:      full mask (a route remembers the whole possibility state),
        kind:      Emphasize,
        strength:  route_pull(usage) — saturating, e.g. u/(u+U₀), capped ≪ 1,
        falloff:   corridor radius,
        source:    AnchorSource::Manual (route-derived; viz-tagged),
    }
```

These are appended to the player's own anchors and flow through the unchanged
order-independent `steer` → `project_plausible` → convergence path.
Consequences, each inherited rather than re-proven:

- **Soft, not replay** (section 13): strength saturates well below 1, so a
  route bends the target toward its recorded signature but can never force it —
  and projection still applies, so a route recorded under old constraints can
  never realize an implausible world.
- **Monotone in usage:** `route_pull` is increasing and bounded — "frequently
  used routes become easier to follow" (Overview) with a ceiling.
- **Composes with everything:** route + player anchors combine
  order-independently (ADR 0011); resonance still gates convergence (ADR 0012);
  a stationary player on a route still sees a still world (ADR 0006).
- **Traversal detection:** when the player passes within the corridor along a
  sufficient fraction of a route's nodes, `usage` increments once per run-leg
  (debounced) and the record is marked dirty.

The route *graph* view (nodes keyed by `signature.seed()`) answers the
inspector's possibility-space queries — "which recorded corridors pass near
this possibility state" — read-only in Phase 5 (§1.4).

### 7.5 Preserves

Creating a preserve snapshots the chosen window's regions as `(coord,
PossibilitySignature::of(current))`, writes the record, and installs each pair
into the `RegionMap` override table. From then on, for an overridden region:

- **Load path:** `current` and `target` come from `signature.dequantize()`
  (not the field sample), `stability = 1`.
- **Retarget/converge:** skipped — anchors, routes, bias, travel, and resonance
  do not move a preserved region (the Overview's preserves; also the simplest
  correct interaction with steering: none).
- **Eviction:** the override survives (it is the persistent fact); the region
  reloads from it identically. Generation then reproduces the preserved
  landscape from the buckets via the unchanged dep-hash → tiles path (ADR
  0008) — bit-identical within a platform, and identical in possibility state,
  dependency hashes, and integer-topology surfaces (drainage, seeds)
  cross-platform, with `f32` tile values remaining per-platform
  float-deterministic exactly as they have been since Phase 2.

Deleting a preserve removes the record and overrides; the regions rejoin
normal steering from their preserved state (continuity: their `current` starts
where the preserve held it; travel-fueled convergence takes over — no snap).

### 7.6 Merge (the server-compatible model)

`Vault::import(bundle)` merges record sets:

- **Union by content id.** An unknown id is inserted whole.
- **Immutable fields are conflict-free by construction:** same id ⇒ same
  immutable fields, because the id *is* their fold (§7.1). A record whose
  recomputed id mismatches its stored id is rejected as corrupt
  (`wer-atlas check` reports it).
- **Mutable fields** (name, journal) resolve by `(sequence, field-hash)` max —
  deterministic, commutative tiebreak. `usage` merges by `max` (idempotent —
  re-importing a bundle never double-counts).

These rules make merge **commutative, associative, and idempotent** — a state
CRDT over records — so bundle exchange needs no coordinator and a future server
is a dumb id-keyed store. The laws are asserted directly on scripted bundles
(§12.3), which is what makes "server-compatible persistence model" a checked
property instead of a slogan.

### 7.7 Save/load orchestration

Every mutating action (`record_*`, usage bump, rename) marks a record dirty;
the session snapshot is refreshed on demand (a key press; later an autosave
cadence) rather than per frame. `flush(budget)` encodes and writes at most
`max_persist_ops` records per frame — persistence obeys temporal budgeting
(section 6.6) like every other subsystem, and a giant import never stalls a
frame. Each key write is atomic at the `Storage` layer (FileStorage:
temp+rename), so a crash mid-flush loses at most un-flushed dirtiness, never
corrupts a record, and the store is always a consistent set of whole records.
Load order is irrelevant by construction: records are independent keys, anchors
combine order-independently (ADR 0011), and overrides are a keyed table.

---

## 8. Scheduling and budgets

### 8.1 Persistence rides the frame, event-driven

No new job type and no scheduler change. Vault work is main-thread (the
`Storage` trait is `&mut self`; the main thread stays the only writer of
everything, preserving the Phase 3 discipline): dirty-marking is O(1) at action
time, `flush` is budgeted (§7.7), record loading happens at open/import (bulk,
off the hot path). Route recording and attraction assembly are O(active nodes)
per frame, capped by config and budget. The generation pipeline sees nothing
new: loaded influence arrives as anchors and overrides before `retarget`, so
the step order (integrate, evict, load, retarget, converge, dispatch, realize)
is unchanged, with `vault.flush` appended after `realize` where a frame's
remaining budget is known.

### 8.2 Budgets and stats

`Budget` gains `max_persist_ops` (records encoded+written per frame) and
`max_route_attraction_nodes` (derived anchors per frame — the steering-side
analogue of `max_resonance_nodes`). `FrameStats` grows `records_flushed`,
`dirty_records`, `persist_bytes`, `route_nodes_active`, and `routes_active` —
the raw material for the panel and the harness. Generation budgets are
untouched: Phase 5 dispatches the same layer jobs with the same costs, only
sometimes because a loaded record moved a target — re-validated by the ledger
staying green (§12.3).

---

## 9. Determinism and versioning

### 9.1 No world-version bump

Phase 5 changes **no generated output for any input**: no layer math, no
hashing, no fold order, no steering math. Loaded records act through the same
target-vector machinery as live actions, so `WORLD_ALGORITHM_VERSION` stays at
**2** and every existing golden fixture stays blessed. (A casual re-bless of
any Phase 2–4 fixture during Phase 5 is a determinism bug, per AGENTS.md.) The
version that *does* move is new and orthogonal: `RECORD_FORMAT_VERSION`, the
first persistence-format axis, with its own golden bytes and migration tests.

### 9.2 What is portable vs presentation-grade vs run-local

- **Portable, golden-fixtured, wasm-parity-tested:** the record codec (byte
  identical for identical records — `record_codec_sample`); content ids;
  `PossibilitySignature` quantize/dequantize/seed; `DiscoveryRecord::to_anchor`
  and steering from it (`shared_steer_sample`); route attraction math and
  `route_difficulty` given records; merge results given bundles.
- **Presentation-grade, per-platform (unchanged from Phase 4):** live capture,
  resonance, habitat-signature *derivation*, `f32` tile values. Which record a
  live action *creates* is per-run/per-platform; what a created record *means*
  is portable — the ADR 0010/0011 boundary, now enforced by the type system
  (shareable records cannot hold floats).
- **Run-local:** the session tier — bit-exact by design, meaningless to share,
  excluded from bundles and merge.

### 9.3 The identity ledger (extended)

Phase 5 adds no integer *world* identity. It adds the portable record surfaces
above, and one new integer vocabulary: `PossibilitySignature::seed`. The
knife-edge residual ADR 0010 documented (f32-reading derivations) is not
removed — it is *routed around*: the atlas never re-derives, it stores. A
shared record's meaning never depends on re-running a presentation-grade
classification on the receiving platform.

### 9.4 Format versioning and migration

Schema changes bump `RECORD_FORMAT_VERSION` and add a pure migration function;
old-version golden bytes are kept and decode-tested forever (§12.1). Additive
room is pre-planned: `RecordKind` is open-ended (future modified-terrain or
structure overrides are new kinds, not schema breaks), and records store the
`TraitCategory` mask alongside domain buckets so the eventual growth of the
possibility vector widens records without invalidating old ones (a v-N reader
maps old 8-domain buckets into the wider vector's corresponding domains).

### 9.5 New ADRs

- **ADR 0013 — Shareable records are quantized at the persistence boundary.**
  Two persistence tiers: run-local session state may carry bit-exact floats;
  shareable records carry only integers (quantized possibility signatures,
  banded strengths/costs, integer positions) and strings. Portability is won at
  record creation, not by making live derivation portable — executing the
  upgrade path ADRs 0010/0011 named. Records mean the same thing on every
  platform; live derivation stays presentation-grade.
- **ADR 0014 — The vault stores deviations, keyed by content-derived ids, with
  CRDT merge laws.** No tiles, organisms, or geometry ever persist; record ids
  fold the immutable integer fields so identical discoveries collide
  world-wide and merge is commutative/associative/idempotent (union by id,
  max-merge mutables). "Server-compatible" is defined as these laws. One-way
  door: anything later wanting to persist generated output (e.g. baked meshes)
  is a cache, not a record, and lives outside the vault.
- **ADR 0015 — Routes attract as derived anchors; attraction is soft and
  saturating.** Route influence reuses the order-independent steering algebra
  (no second steering system), with bounded saturating strength so a route
  biases but never replays, and projection/resonance/travel gates all still
  apply. The route graph is a rebuilt view over records, never a persisted
  structure.

---

## 10. Threading model

Unchanged in kind from Phase 4. The vault, recorder, and attraction assembly
are main-thread; `Storage` implementations may buffer internally but the trait
contract stays synchronous per call and single-writer. Record encode/decode is
pure and `Send`-able — if profiling ever shows `flush` mattering, encoding can
move onto the existing `TaskExecutor` without a design change (writes stay
main-thread). Everything lands and passes the replay under `InlineExecutor`
first, per the established sequencing; `FileStorage` is exercised by
platform-native tests, `MemoryStorage` by everything headless. Nothing in the
neutral crates touches the filesystem: `FileStorage` lives in
`platform-native`, exactly the seam ADR 0002 cut.

---

## 11. Debug visualization and tools

- **Map overlays** (`viz.rs`): a **discovered** dimming layer (seen vs unseen
  regions — the first appearance of the atlas map); **route** polylines with
  brightness = usage and node ticks colored by recorded cost (difficulty reads
  at a glance); **preserve** outlines with a pin glyph. Route-derived anchors
  render distinctly from player anchors in the existing influence channel.
- **Panel**: vault line (records by kind, dirty count, bytes flushed, store
  path); active routes (name, nodes, usage, difficulty); preserve count;
  last-save marker; format/world version of the open store.
- **Shell keys**: save/load session; name-last-capture (placeholder name);
  preserve near window; toggle route recording; toggle route attraction.
- **`wer-inspect --vault X Y`**: for the store, dump the records relevant to a
  position — covering preserve and its buckets, nearby route nodes and their
  signatures, discoveries within range — plus store totals; the persistence
  analogue of `--layers`.
- **`wer-inspect --routes X Y`**: query the route graph around a position in
  *possibility* space: nearest recorded signatures, their routes, difficulty.
- **`wer-atlas`**: `export` / `import` / `check` / `list` (§5.3) — the
  file-based proof of the sharing model.
- **Vault harness** (`wer-vault`): headless runner for the §12.3 scenarios —
  the Phase 5 sign-off tool, alongside the still-passing ledger, ecology, and
  anchor harnesses.

---

## 12. Testing strategy

### 12.1 Golden determinism fixtures

New fixtures, no existing re-blesses (§9.1): **canonical record bytes** (one of
each kind, fixed contents → exact encoded bytes — pins the codec and postcard
behavior); **content ids** for fixed discovery/route/preserve inputs;
`PossibilitySignature` quantize/seed for fixed vectors; `to_anchor` +
steer-from-record output for a fixed record set; route attraction anchors and
`route_difficulty` for a fixed route; merge output for fixed bundles; and
**format-v1 archive bytes** decode-tested forever as the migration floor.

### 12.2 Continuity replay (extend, must stay green)

The Phase 4 script and assertions run unchanged, plus a persistence leg:

- **Save/load equality:** run the script; snapshot mid-run into a
  `MemoryStorage`; continue to the end and record the state hash. Separately:
  reload the snapshot into a fresh `RegionMap`, replay the remaining script,
  and assert the **same state hash** — durability is exact, not approximate.
- **Load is not an event:** immediately after session load and settle, no
  region converges and no layer regenerates beyond what the uninterrupted run
  did at the same point (loading must not manufacture change).
- **Preserve continuity:** a preserve created mid-replay holds its regions
  bit-identical (channel content hashes) for the rest of the run, across
  eviction and reload, while unpreserved neighbors keep transforming; deleting
  it produces no snap (targets resume from the held state).

### 12.3 Vault harness (the Phase 5 success criterion)

Scenario families over scripted journeys (all `MemoryStorage` +
`InlineExecutor`), each machine-checked:

**Durable:**

| Scenario | Expected effect |
|---|---|
| Save → load → settle vs uninterrupted | equal state hashes (§12.2, run headless here too) |
| Crash-consistency | a flush interrupted after any prefix of key writes yields a store that opens clean with whole records only |
| Reopen across "runs" | anchors, discoveries, preserves, routes, seen-set all present and effective after reopen |

**Sparse:**

| Scenario | Expected effect |
|---|---|
| Long journey, `k` actions | store bytes ≤ affine bound in `k` + resident-window size; **zero** keys outside the §6.1 namespace; no tile/organism data anywhere in the store |
| Travel-only journey | only session + seen-set bytes grow; seen-set bytes per region below a small constant |

**Shareable (the social model):**

| Scenario | Expected effect |
|---|---|
| Export from run A, import into run B | imported anchor's steering identical to A's record-steering (golden); imported preserve realizes identical buckets, dep hashes, and channel hashes (same platform) |
| Import order | permuted import order ⇒ identical merged store and identical steering (ADR 0011 cashed in) |
| Merge laws | `merge(A,B) == merge(B,A)`; `merge(merge(A,B),C) == merge(A,merge(B,C))`; `merge(A,A) == A`; usage never double-counts on re-import |
| Corrupt/foreign records | id-mismatch and future-format records rejected with a report, never a panic or partial apply |

**Routes:**

| Scenario | Expected effect |
|---|---|
| Record → follow | near the corridor, the target moves toward the route signature; beyond the corridor, untouched; effect monotone in usage and saturating |
| Soft, not replay | steered target ≠ recorded signature under a conflicting player anchor (routes bias, never force); projection still holds everywhere |
| Difficulty | a route recorded through barren (low-resonance) ground reports higher difficulty than one through dense ground |
| Traversal | re-walking a route increments usage exactly once per leg |

**Precision preserved:** with records loaded (anchors, routes, preserves), the
invalidation ledger, ecology harness, and anchor harness scenarios still pass —
persisted influence obeys exactly the invariants live influence does. Plus a
budget test: a bulk import flushes within `max_persist_ops` per frame and
attraction assembly stays within `max_route_attraction_nodes`.

### 12.4 Unit tests

Record round-trip (encode→decode == identity) per kind; quantize/dequantize
bounds and epsilon; content id excludes mutable fields (rename ⇒ same id) and
covers immutable ones (any immutable change ⇒ new id); `anchor_set_signature`
order-independence; `route_pull` monotone/bounded; migration chain on archived
bytes; `MemoryStorage`/`FileStorage` contract tests (load/store/remove/
contains/`keys_with_prefix`, atomicity for `FileStorage`); override table
survives eviction; future-format refusal.

### 12.5 Native ↔ wasm parity

`platform-web` exports `record_codec_sample()` and `shared_steer_sample()`
(§5.3), pinned to native goldens in the existing parity test. Live vault I/O is
**not** exported — there is no browser storage backend until Phase 7, and the
parity surface is deliberately the *format and the math*, which is exactly what
the browser port will need to interoperate with native-written bundles.

### 12.6 CI

The existing contract, unchanged: fmt, clippy `-D warnings`, native check+test,
wasm32 check of the neutral crates + `platform-web` (now proving
`serde`/`postcard`/record code is wasm-clean). New benches build in CI but are
not timing-gated.

---

## 13. Profiling and metrics

- Per-frame vault time (dirty-mark, flush encode+write), route-recorder time,
  attraction-assembly time and derived-anchor count; import/export wall time
  and bytes for representative bundles.
- Criterion benches: record encode/decode per kind, content-id fold,
  session snapshot of a full resident window, merge of two scripted bundles,
  attraction assembly for a long route. These calibrate `max_persist_ops` and
  `max_route_attraction_nodes`.
- Panel/telemetry grow the §8.2 stats; flush time joins the per-pass breakdown.
- Store telemetry: total bytes by kind — the live view of the sparsity
  guarantee (section 23.6's "continuous memory telemetry", extended to disk).

---

## 14. Native and browser constraints

Restating where Phase 5 stresses standing obligations: all record, route, and
merge code is pure and wasm-clean (CI-enforced; `postcard`/`serde` are
`no_std`); the neutral crates still never touch the filesystem — `Storage`
stays the only door, `FileStorage` lives in `platform-native`, `MemoryStorage`
is allocation-only; the format is byte-oriented k/v with per-record keys, so it
maps onto IndexedDB/OPFS unchanged and supports partial loading (section 18);
all vault work is budgeted and interruptible (crash-consistent per key, §7.7);
no large monolithic allocations (records are small; bundles stream record by
record); and sequence numbers replace wall-clock in all determinism-relevant
paths (no `Date.now` dependence — the shell may stamp non-authoritative
wall-clock metadata only in journal strings).

---

## 15. Risks (mapping section 23)

| Risk | Phase 5 manifestation | Mitigation |
|---|---|---|
| 23.4 Platform divergence | The first on-disk format bakes in native assumptions | Byte k/v records behind `Storage`; codec parity export; no pointers/platform words; browser-compatible by construction; `MemoryStorage` keeps all harnesses platform-free (§6.1, §12.5). |
| 23.5 Determinism drift | Persisted floats or load order perturb the replay; shared records mean different things per platform | Two-tier policy: session floats bit-exact and run-local; shareable records integer-only (ADR 0013); order-independent steer + keyed overrides make load order irrelevant; save/load state-hash equality is a harness gate (§12.2). |
| 23.6 Memory growth | The store grows without bound; seen-set sprawls | Sparse-by-construction records with asserted size bounds (§12.3); seen-set run-length by macro cell; session bounded by the resident window; store telemetry (§13). |
| 23.1 Continuity | A loaded preserve or heavy route yanks targets into a visible cliff | Preserves pin (no convergence at all); route strength saturates ≪ 1 and rides projection + travel/resonance gates; load-is-not-an-event and no-snap-on-delete replay assertions (§12.2). |
| 23.3 Dependency explosion | An import dirties the world wholesale | Records act only through target vectors; regeneration happens only where buckets actually flip; the ledger re-run under loaded records is a harness gate (§12.3). |

The phase-specific risk: **format lock-in** — the first persisted byte is a
compatibility promise, and casual schema churn would either strand player data
or breed silent corruption. Mitigation: the envelope + `RECORD_FORMAT_VERSION`
from day one, migrations as pure tested functions, archived old-version golden
bytes decoded forever, refusal (never guessing) on future versions, and
`wer-atlas check` as the user-facing validator — the same
fixture-or-it-didn't-happen discipline the determinism goldens established.

---

## 16. Incremental milestones

Each keeps CI green (including wasm32), keeps the continuity replay, the
invalidation ledger, the ecology harness, and the anchor harness passing, and
preserves the crate-boundary and determinism invariants. No milestone
re-blesses a Phase 2–4 fixture (§9.1).

- **M1 — Record schema + codec.** `record.rs` (envelope, signature, all record
  kinds, content ids, encode/decode), `serde` + `postcard` into the workspace,
  ADR 0013; canonical-bytes and content-id goldens; `record_codec_sample` +
  `shared_steer_sample` parity exports; round-trip and id unit tests. Pure
  world-core, no runtime change. *Exit:* byte-stable codec, parity native ==
  wasm, record→anchor steering within quantization epsilon of live.
- **M2 — The vault + session durability.** `Storage::keys_with_prefix`,
  `MemoryStorage`, `vault.rs` (namespace, dirty queue, budgeted flush,
  open/version check), `FileStorage` + save/load keys in the shell, session
  snapshot/restore including `RegionMap::restore_region`; ADR 0014. *Exit:*
  save→load→settle state hash equals the uninterrupted run's (replay leg,
  §12.2); crash-consistency test passes; store telemetry in the panel.
- **M3 — Named discoveries + shared anchors + atlas bundles.** `record_discovery`
  wired to capture; discovery→anchor on load; `AtlasBundle`, `Vault::import`/
  `export` with the merge laws; `wer-atlas export|import|check|list`; seen-set
  recording + discovered overlay. *Exit:* a bundle exported from run A steers
  run B identically (golden); merge laws hold; import order irrelevant;
  re-import never double-counts.
- **M4 — Preserves.** `record_preserve`, the possibility-override table in
  `stream.rs` (load-from-buckets, pin, skip retarget/converge, survive
  eviction), preserve key + overlay, no-snap deletion. *Exit:* a preserved
  window holds bit-identical channel hashes across travel, steering, eviction,
  and reload while neighbors transform; an imported preserve realizes identical
  buckets and dep hashes.
- **M5 — Routes.** `route.rs` in both crates (recorder, graph view, attraction
  as derived anchors, usage, difficulty), ADR 0015; recording/attraction keys +
  route overlay; `wer-inspect --vault` / `--routes`;
  `max_route_attraction_nodes`. *Exit:* record→follow attraction is soft,
  corridor-bounded, monotone-saturating in usage; difficulty reflects recorded
  resonance; traversal increments usage once per leg.
- **M6 — Vault harness + sign-off.** The `wer-vault` harness covering every
  §12.3 family (durable / sparse / shareable / routes / precision-preserved);
  benches calibrating `max_persist_ops` and attraction assembly; AGENTS.md and
  README command updates. *Exit:* every §12.3 property holds; the ledger,
  ecology, and anchor harnesses pass *with records loaded*.

**Phase 5 is done when** M1–M6 are complete, CI is green (native + wasm32,
goldens, parity, continuity replay, all four harnesses), and the success
criterion holds with evidence: exploration leaves durable structure (save→load
is state-hash exact), shareable structure (bundles merge lawfully and steer
identically wherever they land), and sparse structure (the store holds
quantized intents and identities — never generated geometry — with
machine-checked size bounds), while routes attract softly through the existing
steering algebra and continuity, determinism, and invalidation precision stay
exactly as tight as the earlier phases left them — the persistence and sharing
foundation Phase 6's scale work and Phase 7's browser runtime will build on.
