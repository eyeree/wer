# 0028. Shared viewer host and one-world multi-view presentation

Date: 2026-07-13

## Status

Accepted

## Context

The native and browser shells currently share world generation and most POV
geometry, but each shell separately owns input bindings, viewer state,
top-down map presentation, inspection, and panel-data construction. The two
implementations have consequently drifted: browser buttons bypass keyboard
handling through string commands, the browser map exposes a reduced set of
channels and overlays, and browser POV movement can diverge from the position
used to stream the world.

Adding side-by-side Map and POV presentation makes those duplicate authorities
unsafe. Two independent viewer updates could apply travel twice, stream around
different centers, or present panes from different world states. Likewise,
calling two whole-surface renderer entry points cannot produce a correct Split
frame because each entry point independently acquires, clears, submits, and
presents the surface.

ADR 0002 keeps generation and runtime authority platform-neutral and forbids
platform dependencies from flowing into `world-core` or `world-runtime`. ADR
0017 makes GPU rendering derived presentation and prohibits live readback. ADR
0021 permits only a narrow, file-bound headless capture exception. The viewer
alignment must preserve all three boundaries while allowing native and web to
share presentation behavior.

## Decision

Cross-platform viewer behavior lives in a new platform-neutral
`viewer-host` crate. It owns semantic viewer actions, normalized input state,
bindings, layout and focus, the exploration/view controller, map composition
and atlas preparation, CPU-side inspection, and the semantic information-panel
model. Native and browser crates remain thin adapters for raw environment
events, storage/executor services, surface creation, application lifecycle,
and final bitmap or DOM panel rendering.

Raw winit and DOM events are normalized before they reach viewer behavior. One
binding registry maps normalized events to an ordered queue of typed semantic
actions and continuous frame intent. Keyboard, pointer, wheel, browser buttons,
and future controller adapters all enter this same ordered consumer; platform
adapters do not directly mutate viewer state.

Map and POV are presentations of one traveler and one world state. Each
logical frame reduces service responses, actions, and continuous intent in a
defined order, computes travel once, and calls `RegionMap::update` once. Split
mode builds both presentation packets from that same post-update state; panes
cannot own independent streaming centers.

The renderer prepares resources separately from surface presentation and
records every visible pane into one surface frame. A frame performs one surface
acquire, one ordered submission sequence, and one present whether it contains
Map, POV, or both. Renderer-facing values remain in `renderer`; `renderer` does
not depend on `viewer-host`, avoiding a dependency cycle through `pov-host`.

Map and POV inspection use CPU-authoritative or CPU presentation geometry: map
tiles and realized organisms for Map, and the resident terrain lattice plus
renderer-ready organism geometry for POV. Inspection never reads back GPU
color, depth, or compute output and never becomes generation, identity,
steering, or persistence authority. ADR 0021's file-bound debug capture remains
the only permitted renderer readback.

## Consequences

Native and browser behavior can be compared with normalized input and
controller traces instead of maintaining two viewer specifications. Buttons
and physical controls cannot silently take different reducer paths, and help
metadata can derive from the binding registry.

Split presentation cannot apply travel twice or show panes centered on
different worlds. Its renderer work must be planned as one frame rather than
assembled from two whole-surface convenience calls, which requires a staged
renderer API and explicit pane rectangles.

`viewer-host` may depend on `world-core`, `world-runtime`, `pov-host`, and
renderer value/upload types, but it may not use winit, web/DOM APIs,
filesystems, sockets, or native thread creation. Core crates never depend back
on it, and `renderer` never depends on it.

This refactor may change input routing and derived presentation pixels. It does
not change world generation, stable identity, layer algorithms, or persisted
records; their version numbers and determinism fixtures remain unchanged.
