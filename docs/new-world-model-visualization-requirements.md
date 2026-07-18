# New World Model Visualization Requirements

## Status and purpose

This document derives Visualization requirements from the
[World Explorer Conceptual Model](conceptual-model.md). It describes what a Model and its
Realization need to make knowable, and what a compatible Visualization needs to preserve, so that
the same world can support both top-down and embodied experiences.

This is the **downstream traceability source** for later Visualization contracts, architecture
decisions, implementation plans, and verification plans. Those later documents may choose concrete
representations, but they should trace their obligations and deliberate exceptions to the stable
requirement identifiers in this document.

The requirements are aspirational and model-independent. They do not describe only the current
prototype, and they do not require every Model to represent a planet, a volumetric world, complex
ecology, or every optional concept in the [project overview](Infinite_World_Exploration_Project_Overview.md).
A compatible pair may support a smaller declared capability set while preserving the invariants
that apply to it.

## Reading this document

The key words **shall**, **shall not**, **must**, and **must not** identify normative requirements.
Every normative system requirement has a stable identifier of the form `VIZ-Rnn`. Requirements are
grouped by subject for readability; their identifiers remain stable if sections are reorganized.

Sections titled **Illustrative approaches (non-normative)** contain examples only. They are not
preferred designs, acceptance criteria, or hidden interface requirements. In particular, examples
of spatial subdivision, procedural reconstruction, or level of detail do not select an API, data
layout, rendering technique, or platform.

Responsibility labels used in requirement titles have these meanings:

- **Model/Realization** identifies information whose stable meaning originates in the Model and is
  conveyed by the Realization.
- **Visualization** identifies interpretation, simulation, presentation, or continuity work owned
  by the Visualization.
- **Cross-cutting** identifies an invariant that constrains both sides or their compatibility.

“Available” means that the information can be obtained with its declared meaning when needed. It
does not imply that the whole world is eagerly materialized or held in memory.

## 1. Foundational separation and traceability

### Normative requirements

**VIZ-R01 — Downstream traceability source.** Every later Visualization contract, architecture
decision, implementation plan, and verification obligation shall cite the applicable requirement
IDs from this document; a deliberate exception shall identify the affected IDs and its rationale.

**VIZ-R02 — Conceptual-model alignment.** Interpretations of Model, Model State, Possibility,
Realization, Visualization, World Space, View Space, Traveler, Exploration, Egress, and simulation
shall remain consistent with the [conceptual model](conceptual-model.md).

**VIZ-R03 — Representation neutrality.** Satisfaction of these requirements shall not depend on a
particular programming language, memory layout, process boundary, transport, storage format, or
rendering backend.

**VIZ-R04 — Responsibility separation.** The Model/Realization shall own canonical world meaning;
the Visualization shall own presentation and transient simulation; neither side shall silently
reassign the other's responsibility.

**VIZ-R05 — Canonical/presentation distinction.** Every presented entity or phenomenon that can
be inspected, remembered, shared, or used to guide Egress shall retain a distinguishable canonical
model meaning even when its presentation varies.

**VIZ-R06 — One-world interpretation.** A Visualization shall interpret one Model State as one
complete physical world, not as a collection of independently chosen per-region worlds.

**VIZ-R07 — One current Possibility point.** Retained regional history and continuity treatments
shall not imply that the Traveler has multiple canonical current points in Possibility.

**VIZ-R08 — Independent spaces.** Possibility Space, World Space, and View Space shall remain
distinct, and no visual projection or travel aid shall collapse their locations or distance
semantics into one metric.

**VIZ-R09 — Simulation isolation.** Visualization simulation shall not alter Model State,
Reachable Possibility, or canonical model attributes unless a separately defined gameplay rule
explicitly turns an action or observation into Model-directed Egress.

**VIZ-R10 — Coherent experiences.** Concurrent or alternate map and embodied presentations shall
describe the same canonical Traveler location, Model State, relevant model time, simulation time,
and post-transition world history.

## 2. Capability and semantic sufficiency

### Normative requirements

**VIZ-R11 — Capability declaration (Model/Realization).** A Model shall identify the world,
environmental, ecological, temporal, observation, and transition capabilities its Realizations
provide, including material limitations on their extent or fidelity.

**VIZ-R12 — Required versus optional meaning (Cross-cutting).** A compatible Model and
Visualization shall distinguish capabilities essential to a usable experience from optional
capabilities whose absence permits a declared reduced experience.

**VIZ-R13 — Semantic completeness (Model/Realization).** Each supplied capability shall have
enough model-defined meaning for a Visualization to interpret values, categories, relationships,
spatial support, temporal relevance, and validity without relying on visual guesswork.

**VIZ-R14 — Availability semantics (Model/Realization).** The Realization shall distinguish a
meaningful zero or absence from information that is unsupported, outside the representable domain,
not yet refined, invalid, or unavailable because of failure.

**VIZ-R15 — Stable cross-view meaning (Cross-cutting).** A capability consumed by more than one
presentation shall keep the same canonical interpretation in map, embodied, inspection, capture,
and accessibility presentations.

**VIZ-R16 — Materialization independence (Model/Realization).** Whether world information is
precomputed, generated incrementally, reconstructed, or retained shall not change its canonical
meaning for identical Model inputs.

**VIZ-R17 — No invented canonical fallback (Visualization).** When required model information is
missing or unusable, a Visualization shall not present a stylistic estimate as if it were canonical
Model output.

## 3. World Space and geometry

### Normative requirements

**VIZ-R18 — World spatial frame (Model/Realization).** A Realization shall provide an addressable
World Space whose locations have stable meaning within the associated Model State.

**VIZ-R19 — Domain and topology (Model/Realization).** The Realization shall describe the relevant
topology of World Space, including whether it is planar, planetary, bounded, periodic, disconnected,
layered, or otherwise non-Euclidean where those distinctions affect presentation or travel.

**VIZ-R20 — Location interpretation (Model/Realization).** World locations shall have defined
units, dimensionality, orientation, reference origins or surfaces, and validity conditions
sufficient for consistent placement and measurement.

**VIZ-R21 — Distance semantics (Model/Realization).** The Model shall make available the physical
distance or neighborhood meaning needed for Exploration and shall keep it distinct from distance
or adjacency in Possibility.

**VIZ-R22 — Extent and boundary behavior (Model/Realization).** The Realization shall identify
finite extents, wraparound, poles, seams, inaccessible domains, or open-ended continuation wherever
they affect navigation or visual continuity.

**VIZ-R23 — Precision semantics (Cross-cutting).** World positions and derived geometry shall
retain sufficient precision for both intimate movement and long-range exploration, and any loss of
precision shall be bounded and distinguishable from a physical feature.

**VIZ-R24 — Surface geometry (Model/Realization).** A surface-world capability shall provide the
canonical shape, elevation or equivalent displacement, and material domain needed to reconstruct
landforms without prescribing meshes or tessellation.

**VIZ-R25 — Water geometry (Model/Realization).** When water is represented, the Realization shall
provide the spatial meaning of coastlines, water surfaces, depth or volume, connectivity, and flow
features to the fidelity claimed by the Model.

**VIZ-R26 — Non-height-field geometry (Model/Realization).** A Model that permits overhangs,
cavities, floating structures, multiple surfaces, or volumetric media shall identify those
possibilities so a Visualization does not silently flatten them into a contradictory world.

**VIZ-R27 — Geometric discontinuities (Model/Realization).** Cliffs, faults, shorelines, material
boundaries, topology changes, and other intentional discontinuities shall be distinguishable from
sampling gaps, approximation artifacts, and transition errors.

**VIZ-R28 — Planetary and celestial geometry (Model/Realization).** When applicable, the
Realization shall provide the size, shape, rotation, orbital relationships, and spatial frames of
the world and relevant celestial bodies at the canonical fidelity claimed by the Model.

**VIZ-R29 — Spatial relationships (Model/Realization).** Canonical connectivity and containment
relationships that cannot be recovered reliably from local shape alone shall be available when
they matter to drainage, habitats, routes, regions, or feature identity.

**VIZ-R30 — Physical affordances (Model/Realization).** The Realization shall provide enough
canonical physical meaning for the Visualization to avoid contradicting traversability, solidity,
fluid domains, or other exploration-relevant affordances defined by the Model.

**VIZ-R31 — Resolution footprint (Model/Realization).** Geometric information shall carry enough
meaning to determine the area, volume, or feature scale it represents, rather than implying
point-precision where only an aggregate or approximation exists.

**VIZ-R32 — Cross-scale consistency (Cross-cutting).** Coarse and refined geometry for the same
Model inputs shall describe the same canonical world within declared approximation limits; a
change of detail alone shall not create a different landform identity.

**VIZ-R33 — Cross-state spatial correspondence (Model/Realization).** For successive nearby Model
States, the Model shall supply enough correspondence or explicit non-correspondence information for
the Visualization to reason about which locations and geometric features continue through Egress.

### Illustrative approaches (non-normative)

- A world could be described through a height surface, an implicit solid, a volumetric field, a
  collection of connected manifolds, or combinations of those descriptions.
- Large worlds could use local frames over a stable global address system to preserve nearby
  precision without changing the meaning of an Impression location.
- Coarse geometry could summarize a footprint while finer realizations progressively resolve the
  same terrain, shoreline, or cave system.
- Feature correspondence could be inferred from stable feature identities in calm areas and
  explicitly marked as unavailable across bifurcations or topology changes.

## 4. Environmental and semantic fields

### Normative requirements

**VIZ-R34 — Environmental capability inventory (Model/Realization).** A Realization shall identify
which environmental domains it defines, such as geology, terrain, climate, hydrology, soils,
atmosphere, biome, vegetation, radiation, or other world-specific phenomena.

**VIZ-R35 — Field semantics (Model/Realization).** Every canonical environmental field shall have
a defined physical or model meaning, units or category interpretation where applicable, expected
range, and rules for exceptional values.

**VIZ-R36 — Spatial and temporal support (Model/Realization).** Each field shall identify the
location, region, layer, volume, interval, phase, or aggregate to which a value applies.

**VIZ-R37 — Continuous and categorical meaning (Model/Realization).** The Realization shall
distinguish continuous quantities, classifications, ordered bands, probabilities, counts, and
identities so the Visualization does not interpolate or aggregate them incorrectly.

**VIZ-R38 — Canonical versus derived fields (Cross-cutting).** Model-defined fields shall remain
distinguishable from Visualization-derived conveniences such as shading, stylized color,
procedural surface noise, visibility, or transient weather particles.

**VIZ-R39 — Relationships and constraints (Model/Realization).** When independent presentation of
fields would create an implausible contradiction, the Realization shall expose their relevant
relationships, dependencies, or joint constraints.

**VIZ-R40 — Geological meaning (Model/Realization).** A geology capability shall provide the
canonical substrate, structure, age or process distinctions, and feature relationships that the
Visualization claims to depict or expose through inspection.

**VIZ-R41 — Landform meaning (Model/Realization).** Terrain shape shall be accompanied by the
canonical classifications or derived measures needed to distinguish landforms, slope conditions,
erosional features, and other semantically observable terrain properties.

**VIZ-R42 — Climate meaning (Model/Realization).** A climate capability shall distinguish
long-term or phase-dependent environmental conditions from momentary Visualization-simulated
weather, and shall describe the temporal basis of canonical climate values.

**VIZ-R43 — Hydrological meaning (Model/Realization).** A hydrology capability shall make canonical
flow direction, accumulation, drainage connectivity, water regime, and relevant temporal variation
available wherever those properties are presented or inspected.

**VIZ-R44 — Soil and substrate meaning (Model/Realization).** A soil or substrate capability shall
provide the composition, depth, moisture, fertility, stability, or other canonical properties on
which visible environments and ecological constraints depend.

**VIZ-R45 — Biome and vegetation meaning (Model/Realization).** Broad environmental communities,
vegetation structure, cover, biomass, and succession state shall be semantically related to their
supporting climate, terrain, water, and soils when the Model represents those relationships.

**VIZ-R46 — Atmospheric and illumination context (Model/Realization).** When atmosphere, star,
moon, tide, or orbital cycles have canonical environmental consequences, the Realization shall
provide the relevant state and relationships independently of any chosen sky or lighting style.

**VIZ-R47 — Boundaries and mixtures (Model/Realization).** The Realization shall distinguish sharp
boundaries, gradual transitions, mixtures, mosaics, and unresolved classification so the
Visualization does not invent uniform regions where the Model defines heterogeneity.

**VIZ-R48 — Cross-scale environmental consistency (Cross-cutting).** Aggregated and refined field
values shall preserve declared meanings and relationships across scale, with any non-conservative
or scale-dependent interpretation identified.

### Illustrative approaches (non-normative)

- A map could present separate thematic interpretations of elevation, drainage, temperature,
  substrate, vegetation, or habitat while an embodied view combines them into one scene.
- A categorical biome description could be accompanied by mixture proportions near an ecotone
  rather than forcing a single label at every scale.
- Canonical climate could establish seasonal envelopes while deterministic Visualization
  simulation supplies clouds, gusts, precipitation events, and surface motion within them.
- Dependency information could help a Visualization preserve correlations such as wet soils near
  drainage features or vegetation structure appropriate to available water.

## 5. Ecology, species, and organisms

### Normative requirements

**VIZ-R49 — Ecosystem structure (Model/Realization).** An ecology capability shall provide the
canonical species, guilds or equivalent actors, niches, distributions, and relationships needed to
interpret the ecosystem rather than only an unlabelled collection of visual organisms.

**VIZ-R50 — Species identity (Model/Realization).** Canonical species or equivalent lineage
identities shall be stable for identical Model inputs and shall remain distinct from a
Visualization's asset, rig, sound, or behavioral-presentation identity.

**VIZ-R51 — Canonical organism attributes (Model/Realization).** The Model shall define which
organism attributes are canonically observable and portable across compatible Visualizations,
including their meanings and valid variation within a species or lineage.

**VIZ-R52 — Niche and habitat constraints (Model/Realization).** Ecological actors shall have
enough canonical habitat, resource, tolerance, and spatial constraints to prevent their visual
manifestations from contradicting the represented ecosystem.

**VIZ-R53 — Ecological relationships (Model/Realization).** Trophic, competitive, mutualistic,
reproductive, or other relationships that materially define the ecosystem shall be available with
their direction and semantic meaning.

**VIZ-R54 — Distribution and prevalence (Model/Realization).** The Realization shall describe the
spatial distribution and prevalence of ecological actors at meaningful scales, including the
difference between suitable habitat, likely presence, and canonical absence.

**VIZ-R55 — Abundance semantics (Model/Realization).** Counts, densities, biomass, carrying
capacity, occupancy, or qualitative abundance shall be distinguishable, and the area, volume, or
time interval to which an abundance value applies shall be known.

**VIZ-R56 — Ecological time (Model/Realization).** Canonical seasonality, life-cycle phase,
succession, migration, dormancy, or other model-time dependencies shall be available when they
change which organisms or attributes should be manifested.

**VIZ-R57 — Representative individual realization (Model/Realization).** The Model shall provide
enough canonical variation and population context for a Visualization to construct deterministic
individual manifestations representative of a species without mistaking stylistic variation for a
new canonical species.

**VIZ-R58 — Individual identity boundary (Cross-cutting).** A presented individual shall be treated
as a permanent canonical entity only when the Model supplies that identity; otherwise its transient
simulation identity shall not be recorded as though it were portable model identity.

**VIZ-R59 — Placement and behavior constraints (Cross-cutting).** Organism placement and simulated
behavior shall respect canonical habitat, physical, temporal, abundance, and ecological
constraints, while leaving transient paths and actions to the Visualization.

**VIZ-R60 — Aggregate/individual consistency (Visualization).** The number and diversity of
manifested individuals shall be a declared sampling or representation of canonical population
meaning and shall not falsely imply local abundance or absence.

**VIZ-R61 — Portable ecological observation (Cross-cutting).** Inspection and Impressions of an
organism or ecological feature shall preserve canonical identity and observable attributes across
compatible Visualizations even when individual appearance and animation differ.

### Illustrative approaches (non-normative)

- A Realization could describe a habitat-level roster and relationships, then supply deterministic
  variation from which nearby individuals are manifested.
- A sparse embodied population could stand in for a much larger canonical density if inspection
  and overview presentations retain the density's actual meaning.
- Permanent named organisms could use Model identities, while ordinary background individuals
  could be deterministic simulation manifestations without portable individual identity.
- Food-web or habitat summaries could support map-scale ecology while morphology, motion, and sound
  generators support the embodied manifestation of the same species.

## 6. Canonical observation and inspection

### Normative requirements

**VIZ-R62 — Canonical observation source (Model/Realization).** The Realization shall make
canonical observable facts available independently of rendered pixels, generated audio, animation
poses, or transient simulation accidents.

**VIZ-R63 — Subject resolution (Model/Realization).** At an inspectable World Space location and
relevant time, the Realization shall make it possible to distinguish the canonical subject or
subjects, their spatial support, and ambiguous or overlapping candidates.

**VIZ-R64 — Observable attribute meaning (Model/Realization).** Every observable attribute shall
have stable semantics, including whether it describes an entity, population, field, relationship,
region, temporal phase, or aggregate.

**VIZ-R65 — Observation context (Cross-cutting).** A canonical observation used outside the
immediate presentation shall retain the associated Model identity and version, Model State,
World Space location, relevant model time, and subject identity or support.

**VIZ-R66 — Canonical relationship inspection (Model/Realization).** Relationships needed to
understand a subject, such as species membership, drainage membership, habitat association, or
feature containment, shall be inspectable when the Model declares them observable.

**VIZ-R67 — Presentation-independent values (Visualization).** A Visualization shall not substitute
screen color, mesh choice, apparent size, animation state, visibility, or other presentation
properties for a canonical observation unless the subject is explicitly a Visualization property.

**VIZ-R68 — Cross-experience agreement (Cross-cutting).** Map selection, embodied inspection, and
other compatible inspection experiences shall report the same canonical subject and values for the
same Model State, location, model time, and refinement quality.

**VIZ-R69 — Observation quality (Model/Realization).** An observation shall carry enough quality,
uncertainty, resolution, and provenance meaning to prevent an approximation or aggregate from
being recorded as exact individual fact.

**VIZ-R70 — Knowledge boundaries (Visualization).** Presentation choices may limit what the
Traveler perceives, but they shall not silently change canonical truth; any gameplay rule that
restricts discoverable information shall remain distinct from data absence or Model uncertainty.

**VIZ-R71 — Impression sufficiency (Cross-cutting).** Canonical observations eligible for an
Impression shall supply enough identity, context, and attributes for the Impression to retain its
model meaning in another compatible Visualization.

**VIZ-R72 — Yearning eligibility (Model/Realization).** The Model shall identify which observed
attributes can meaningfully influence Egress and how their canonical semantics relate to Influence
and prevalence Scope, without making presentation-only attributes steer Possibility.

**VIZ-R73 — Repeatable inspection (Cross-cutting).** Re-inspecting identical Model inputs at the
same location, relevant model time, and declared refinement shall reproduce the same canonical
observation regardless of view or resource tier.

## 7. Top-down map experience

### Normative requirements

**VIZ-R74 — Overview capability (Visualization).** A compatible top-down experience shall present
enough World Space context to survey geography and environments beyond the immediate embodied
vicinity without redefining the world as a flat View Space image.

**VIZ-R75 — Spatial faithfulness (Cross-cutting).** Map placement, orientation, connectivity, and
distance shall follow declared World Space topology and projection semantics, and material
projection distortion shall not be presented as a physical property.

**VIZ-R76 — Multi-scale coverage (Cross-cutting).** The map experience shall preserve meaningful
context across its supported extents, from local terrain and organisms to regions or planetary
structure, using only detail justified at each scale.

**VIZ-R77 — Thematic semantics (Visualization).** Environmental and ecological map presentations
shall remain traceable to canonical fields, features, relationships, or aggregates and shall keep
stylistic emphasis distinct from Model magnitude or certainty.

**VIZ-R78 — Aggregation honesty (Visualization).** Downsampling, clustering, symbol selection, and
feature omission shall not imply false precision, false absence, or a changed canonical identity.

**VIZ-R79 — Stable map subjects (Cross-cutting).** A canonical feature selected or inspected at one
map scale shall retain its identity at other scales where that feature remains meaningfully
represented.

**VIZ-R80 — State and history legibility (Visualization).** Map presentation shall be able to
distinguish content belonging to the current Realization from retained or reconciled regional
history when that distinction is material to understanding an Egress transition.

**VIZ-R81 — Map uncertainty (Visualization).** Coverage limits, unresolved detail, uncertainty,
stale transition content, and failures shall not be rendered as confidently known canonical map
content.

**VIZ-R82 — Dual-space destinations (Cross-cutting).** When Impressions or Attractors contribute
both a Possibility destination and a World Space location, a map experience shall preserve their
separate precision, distance, and arrival semantics.

### Illustrative approaches (non-normative)

- An overview could combine a base physical map with optional semantic interpretations for climate,
  drainage, ecology, transition provenance, or uncertainty.
- Different projections could serve local, regional, and planetary contexts while disclosing seams
  or distortion through presentation rather than changing World Space.
- Clusters, density surfaces, or representative symbols could summarize organisms at broad scales;
  their meaning could remain population-level rather than individual-level.
- Transition history could appear as a provenance interpretation that is separate from canonical
  current-state environmental values.

## 8. Embodied and point-of-view experience

### Normative requirements

**VIZ-R83 — Embodied world (Visualization).** A compatible embodied experience shall construct an
interactive three-dimensional presentation in which the Traveler can perceive and move through
the physical relationships described by the Realization.

**VIZ-R84 — Metric correspondence (Cross-cutting).** Presented scale, relative placement, horizon,
surface shape, and movement shall correspond to World Space closely enough that exploration,
inspection, and Impression locations remain meaningful.

**VIZ-R85 — Physical interaction (Visualization).** Collision, grounding, fluid entry, occlusion,
and other embodied affordances shall not materially contradict canonical geometry and physical
domains claimed by the Model.

**VIZ-R86 — Near-field semantic coherence (Visualization).** Added surface detail, materials,
vegetation, weather, and ambient effects shall remain compatible with the local canonical geology,
terrain, water, climate, soils, biome, and time.

**VIZ-R87 — Organism manifestation (Visualization).** Embodied organisms shall preserve canonical
species meaning, observable traits, habitat constraints, and abundance semantics while allowing
Visualization-defined form, animation, sound, and transient behavior.

**VIZ-R88 — Far-field consistency (Visualization).** Distant terrain, water, atmosphere,
vegetation, and ecological cues shall summarize the same Realization as nearby detail and shall not
turn refinement boundaries into apparent world boundaries.

**VIZ-R89 — View-space isolation (Visualization).** Camera position, orientation, projection,
display transform, and presentation-specific offsets shall remain View Space concerns and shall not
replace canonical World Space addresses.

**VIZ-R90 — Embodied inspection parity (Cross-cutting).** Embodied picking or inspection shall use
the same canonical observation meaning as map inspection, subject to honestly stated differences
in spatial support and refinement.

**VIZ-R91 — Embodied continuity (Visualization).** Egress, refinement, resource changes, and
simulation updates shall avoid unexplained popping, teleportation, or global replacement of nearby
canonical subjects where continuity information permits reconciliation.

### Illustrative approaches (non-normative)

- The Visualization could derive fine surface form and material variation deterministically from
  canonical landform and substrate properties.
- Distant population cues could use vegetation structure, calls, tracks, or sparse representatives
  rather than manifesting every canonical organism.
- A local presentation frame could move with the Traveler to maintain numeric precision while all
  inspectable positions retain stable World Space meaning.
- Audio, haptics, color alternatives, and semantic descriptions could provide additional embodied
  interpretations without changing canonical observations.

## 9. Model time and Visualization simulation time

### Normative requirements

**VIZ-R92 — Canonical temporal information (Model/Realization).** The Realization shall identify
which world properties depend on model time and shall provide the cycles, phases, epochs, initial
conditions, or temporal relationships needed to interpret those properties.

**VIZ-R93 — Time-domain separation (Cross-cutting).** Canonical model time, Visualization
simulation time, and external wall-clock time shall remain distinguishable, even when a chosen
experience maps them at the same rate.

**VIZ-R94 — Temporal mapping (Visualization).** The Visualization shall define enough of the
mapping between model-time conditions and simulation time to reproduce time-dependent observable
presentation for identical declared inputs.

**VIZ-R95 — Cycle coherence (Cross-cutting).** Diurnal, seasonal, tidal, orbital, ecological, and
other supported cycles shall retain their Model-defined relationships when accelerated, slowed,
paused, or sampled by the Visualization.

**VIZ-R96 — Deterministic simulation (Visualization).** Identical Visualization definition and
version, parameters, Realization, World Space location, simulation time, and other declared
deterministic inputs shall reproduce the same observable transient state.

**VIZ-R97 — Supported temporal movement (Visualization).** Pause, rate change, jump, or reversal
shall be applied only where the Visualization can preserve the declared temporal semantics; an
unsupported operation shall not fabricate canonical past or future state.

**VIZ-R98 — Temporal addressability (Cross-cutting).** An Impression shall retain relevant model
time, simulation time, or canonical temporal phase whenever omitting it could identify a different
subject or observation.

**VIZ-R99 — Temporal refinement (Cross-cutting).** Increasing temporal detail or simulation
frequency shall preserve canonical phase and event meaning within declared error and shall not
silently shift the Traveler in Possibility.

**VIZ-R100 — Time during Egress (Cross-cutting).** A transition shall identify how model-time
conditions and simulation time relate across successive Realizations so change caused by time is
distinguishable from change caused by movement in Possibility.

## 10. Egress transition and continuity information

### Normative requirements

**VIZ-R101 — No global-reload experience (Visualization).** Movement through nearby reachable
Model States shall be presented as continuous travel rather than a portal, global scene reset, or
simultaneous unexplained replacement of the visible world.

**VIZ-R102 — Related neighboring worlds (Model/Realization).** The Model shall make nearby states
produce related Realizations where practical and shall not require the Visualization to infer all
continuity solely from unrelated snapshots.

**VIZ-R103 — Transition path context (Model/Realization).** A transition shall provide enough
meaning about the ordered path through Model States to distinguish intermediate Realizations from
an arbitrary blend between only the endpoints.

**VIZ-R104 — Regional realization provenance (Model/Realization).** World content used during
Egress shall retain the Model State or transition context from which it was canonically realized,
including retained nearby history.

**VIZ-R105 — Feature correspondence (Model/Realization).** Stable entities, fields, and geometric
features shall expose correspondence across successive Realizations where the Model can define it;
unresolved or nonexistent correspondence shall be explicit.

**VIZ-R106 — Continuity risk (Model/Realization).** The Model shall make available known
sensitivity, chaotic divergence, threshold crossings, topology changes, or other conditions that
make nearby states produce materially discontinuous results.

**VIZ-R107 — Change classification (Model/Realization).** Transition information shall distinguish
meaningful canonical change from refinement, stale retained content, simulation evolution, and
presentation-only change.

**VIZ-R108 — Retained-history sufficiency (Cross-cutting).** Enough local realization history
shall be identifiable to reconcile already encountered or nearby content with newly encountered
content without reinterpreting history as the current Model State.

**VIZ-R109 — Canonical-state authority (Cross-cutting).** Continuity treatments shall preserve one
authoritative current Possibility point and shall not feed blended presentation values back into
the Model as a new canonical state.

**VIZ-R110 — Address preservation (Cross-cutting).** Blending, morphing, preservation, relocation,
or replacement used for continuity shall not silently change the Traveler's canonical World Space
location or the address of an Impression.

**VIZ-R111 — Domain-specific reconciliation (Visualization).** Geometry, water, atmosphere,
vegetation, ecology, organisms, simulation, and Builds may require different continuity treatment,
but their combined presentation shall not assert mutually contradictory canonical states.

**VIZ-R112 — Topology-change handling (Cross-cutting).** When correspondence cannot preserve a
feature through a split, merge, appearance, disappearance, or topology change, the transition shall
remain deterministic and shall expose enough provenance to avoid a false identity claim.

**VIZ-R113 — Reachability and Resonance authority (Model/Realization).** Information about
Reachable Possibility, local continuation support, or Resonance shall originate from the Model and
Traveler interaction, and Visualization choice or hardware shall not alter it.

**VIZ-R114 — Exploration/Egress separation (Cross-cutting).** A continuous experience may
coordinate World Space travel with Egress, but transition information shall preserve their separate
paths, progress, rates, and arrival conditions.

**VIZ-R115 — Reproducible transition history (Cross-cutting).** When retained history affects the
observable presentation, its canonical provenance and deterministic transition inputs shall be
sufficient to reproduce that presentation under the same Visualization definition and parameters.

### Illustrative approaches (non-normative)

- Terrain might preserve a previously traversed corridor, vegetation might change gradually by
  cohorts, and transient organisms might disperse or be replaced according to separate policies.
- Stable identities could support direct feature tracking; fields without identity could use
  correspondence confidence over shared spatial support.
- The Model could flag a drainage bifurcation, biome threshold, or lineage replacement as a
  high-risk transition so the Visualization allocates a longer reconciliation interval.
- Newly encountered content could follow later states on the Egress path while a bounded nearby
  history region preserves what the Traveler has already seen.
- A dual-space journey could coordinate rates so World Space and Possibility arrival feel related
  without converting one distance into the other.

## 11. Uncertainty, error, and refinement

### Normative requirements

**VIZ-R116 — Distinct information states (Model/Realization).** Canonical absence, uncertainty,
approximation, unresolved refinement, stale transition content, unsupported capability,
out-of-domain location, and generation failure shall be distinguishable.

**VIZ-R117 — Uncertainty source (Model/Realization).** Uncertain information shall identify whether
the uncertainty is inherent to the Model, caused by finite representation, caused by incomplete
realization, or introduced by Visualization sampling or simulation.

**VIZ-R118 — Quality bounds (Model/Realization).** Approximate information shall provide a useful
statement of accuracy, confidence, range, category stability, or other quality bound appropriate to
its semantics.

**VIZ-R119 — Uncertainty support (Model/Realization).** Quality and uncertainty shall apply to a
known spatial, temporal, entity, field, or transition support rather than appearing as a context-free
global quality label.

**VIZ-R120 — Refinement meaning (Model/Realization).** Available refinement levels shall identify
which semantics become more precise, which new canonical details may appear, and which properties
are invariant across refinement.

**VIZ-R121 — Identity through refinement (Cross-cutting).** Refinement alone shall preserve stable
canonical identities and addresses; if refinement legitimately resolves one aggregate into several
subjects, their relationship to the aggregate shall be retained.

**VIZ-R122 — Correction versus world change (Cross-cutting).** A corrected approximation or
regenerated failure shall be distinguishable from change due to model time, simulation time,
World Space movement, or Egress.

**VIZ-R123 — Honest presentation (Visualization).** Visual prominence, smoothness, opacity,
precision, or detail shall not overstate the certainty, freshness, support, or canonical fidelity of
the underlying information.

**VIZ-R124 — Observation-quality preservation (Cross-cutting).** Inspection and Impression capture
shall retain material uncertainty and refinement context so a coarse inference cannot silently
become an exact portable fact.

**VIZ-R125 — Fallback separation (Visualization).** Placeholder geometry, generic organisms,
cached imagery, or other continuity and failure fallbacks shall remain distinguishable from valid
canonical Realization content.

**VIZ-R126 — Local failure containment (Visualization).** A failure to realize or present one
region, capability, refinement, or optional phenomenon shall not corrupt unrelated canonical state
or imply a different point in Possibility.

### Illustrative approaches (non-normative)

- Quality could be expressed through error ranges, confidence bands, stable-category guarantees,
  source resolution, or a statement that only aggregate meaning is available.
- A coarse terrain footprint could guarantee mean elevation and drainage class while deferring
  exact ridges and small channels to later refinement.
- An unresolved map region could use a neutral incomplete treatment rather than a plausible but
  fabricated biome color.
- Inspection could distinguish “no organisms,” “none manifested at this density,” “not yet
  resolved,” and “ecology unsupported.”

## 12. Identity, provenance, and determinism

### Normative requirements

**VIZ-R127 — Model identity (Model/Realization).** Every Realization shall be attributable to a
distinguishable Model definition and version whose identity changes when canonical interpretation
or deterministic output changes incompatibly.

**VIZ-R128 — Visualization identity (Visualization).** Reproduction of a particular presentation
shall be attributable to a distinguishable Visualization definition and version plus all material
presentation and simulation parameters.

**VIZ-R129 — Model State identity (Model/Realization).** The Realization shall preserve the exact
Model State or Possibility address needed to identify the complete canonical world, independently
of regional materialization and continuity history.

**VIZ-R130 — Semantic vocabulary identity (Model/Realization).** Canonical attribute, category,
unit, relationship, and capability meanings shall be versioned or otherwise distinguishable when
their interpretation changes.

**VIZ-R131 — Entity identity (Model/Realization).** Canonical features, species, relationships, and
other addressable subjects shall have stable identities or stable identifying context appropriate
to their claimed lifetime and scope.

**VIZ-R132 — Identity scope (Cross-cutting).** Global, Model-State-local, region-local,
time-dependent, transition-local, and simulation-only identities shall be distinguishable so a
short-lived manifestation is not treated as a permanent world entity.

**VIZ-R133 — Spatial and scale stability (Cross-cutting).** Repartitioning, rematerialization,
level-of-detail changes, caching, or view changes shall not alter canonical identity for unchanged
Model inputs.

**VIZ-R134 — Model determinism (Model/Realization).** Identical Model definition and version,
Model State, World Space location, relevant model time, and declared quality shall produce the same
Realization meaning and canonical subjects.

**VIZ-R135 — Visualization determinism (Visualization).** Identical Visualization definition and
version, parameters, Realization, location, simulation time, and required history shall produce the
same observable presentation and transient state.

**VIZ-R136 — Controlled variation (Cross-cutting).** Randomness or procedural variation that can
affect reproducible observation shall derive from declared deterministic inputs; unrecorded host or
execution randomness shall not determine canonical identity or promised presentation.

**VIZ-R137 — Hardware and schedule invariance (Cross-cutting).** Hardware capability, concurrency,
work completion order, caching, and resource tier shall not alter Model output, Reachable
Possibility, Resonance, or canonical observations for identical inputs.

**VIZ-R138 — Content provenance (Model/Realization).** Realized information shall retain enough
provenance to identify its Model version, Model State or transition source, World Space support,
relevant model time, and refinement or approximation status.

**VIZ-R139 — Impression reproduction levels (Cross-cutting).** An Impression shall distinguish the
guarantee of reproducing canonical Model meaning from the stronger, optional guarantee of
reproducing an exact Visualization presentation.

**VIZ-R140 — Versioned change (Cross-cutting).** A change that alters either deterministic mapping
shall receive distinguishable version identity and shall not silently reinterpret an existing
Impression, Build anchor, or canonical observation.

## 13. Scale and performance semantics

### Normative requirements

**VIZ-R141 — Large-world applicability (Cross-cutting).** The information model shall support the
declared World Space extent, including complete planets or mathematically unbounded worlds, without
requiring the entire Realization to be simultaneously materialized.

**VIZ-R142 — Multi-resolution equivalence (Cross-cutting).** Coarse, intermediate, and refined
representations shall preserve the same canonical interpretation within declared quality bounds
and shall not define tier-specific worlds.

**VIZ-R143 — Coverage metadata (Model/Realization).** Materialized information shall have known
World Space and temporal coverage, semantic support, refinement, and freshness sufficient to
combine it without gaps being mistaken for canonical absence.

**VIZ-R144 — Scheduling independence (Cross-cutting).** Prioritization, cancellation, batching,
parallelism, and completion order may affect when detail becomes available but shall not affect its
settled canonical meaning.

**VIZ-R145 — Retention independence (Cross-cutting).** Cache size, eviction, recomputation, and
streaming history shall not change canonical output or identity, except that explicitly retained
transition history may affect presentation as governed by `VIZ-R108` and `VIZ-R115`.

**VIZ-R146 — Resource-tier semantics (Visualization).** Resource tiers may change presentation
density, audiovisual richness, simulation frequency, and refinement latency, but shall not change
Model State, Reachable Possibility, Resonance, canonical geometry, or canonical observation.

**VIZ-R147 — Aggregate conservation (Cross-cutting).** Scale-dependent aggregation shall preserve
declared extensive quantities, intensive quantities, categories, identities, and relationships
according to their semantics rather than applying one aggregation rule to all fields.

**VIZ-R148 — Ecological sampling at scale (Visualization).** Reducing manifested organism count
shall preserve representative diversity and declared abundance meaning and shall not make resource
limits appear to cause canonical extinction or habitat change.

**VIZ-R149 — Long-travel stability (Cross-cutting).** Extended movement, coordinate magnitude,
region turnover, and repeated refinement shall not accumulate unbounded drift in canonical
location, geometry correspondence, time, or identity.

**VIZ-R150 — Stable refinement boundaries (Visualization).** Changes in spatial or temporal detail
shall be reconciled so that partition edges, loading order, and refinement thresholds do not appear
as canonical environmental boundaries.

**VIZ-R151 — Bounded degradation (Visualization).** When available resources cannot sustain the
preferred experience, degradation shall preserve canonical meaning and inspection before optional
presentation detail, and shall make unavailable required fidelity evident.

**VIZ-R152 — Performance/gameplay independence (Cross-cutting).** Performance budgets and
hardware limits shall not decide which Model States are reachable, how Yearnings resolve, or what
canonical Resonance the Traveler has.

## 14. Compatibility, evolution, and validation

### Normative requirements

**VIZ-R153 — Determinable compatibility (Cross-cutting).** It shall be possible to determine before
normal use whether a Model/Visualization pair supports the required geometry, semantic,
ecological, temporal, observation, transition, identity, and quality meanings.

**VIZ-R154 — Semantic compatibility (Cross-cutting).** Matching capability names alone shall not
establish compatibility; the pair shall agree on the material meaning, units, domains, identity
scope, time basis, quality, and invariants of consumed information.

**VIZ-R155 — Required-capability refusal (Visualization).** A Visualization shall decline or enter
a clearly identified reduced experience when it cannot preserve a required Model capability,
rather than silently presenting a contradictory world.

**VIZ-R156 — Optional-capability degradation (Visualization).** Absence of an optional capability
may reduce presentation or simulation richness, but shall not change the canonical interpretation
of the capabilities that remain supported.

**VIZ-R157 — Partial compatibility disclosure (Cross-cutting).** Map-only, embodied-only,
inspection-limited, temporally limited, or otherwise partial compatibility shall be distinguishable
from full compatibility and shall identify which guarantees remain available.

**VIZ-R158 — Evolution declaration (Cross-cutting).** A Model or Visualization update shall
identify whether existing Model States, Impressions, canonical observations, transition histories,
and presentation-reproduction claims remain meaningful.

**VIZ-R159 — Cross-platform compatibility (Cross-cutting).** Compatible native, browser, or future
platform implementations shall preserve canonical semantics and deterministic guarantees even when
available presentation features differ.

**VIZ-R160 — Verifiable obligations (Cross-cutting).** Each downstream design shall define evidence
appropriate to its claimed requirements, including cross-view, cross-scale, cross-time,
cross-transition, cross-tier, and cross-platform comparisons where applicable.

## 15. Builds as optional Visualization content

### Normative requirements

**VIZ-R161 — Optional content boundary (Cross-cutting).** A Build shall remain optional
Visualization content associated with an Impression and shall not become part of Model State,
Possibility, or the canonical natural Realization.

**VIZ-R162 — Dual anchor (Cross-cutting).** A Build shall retain the Model and Model State context
plus World Space location needed to reconstruct it at the intended world and place.

**VIZ-R163 — Authored meaning (Visualization).** Compatible presentation of a Build shall preserve
its declared authored structure, relationships, scale, orientation, and semantic intent while
allowing only the stylistic variation permitted by its own definition.

**VIZ-R164 — Natural/build distinction (Visualization).** Inspection, continuity, and map or
embodied presentation shall keep Build content distinguishable from canonical natural features and
from Visualization-generated decoration.

**VIZ-R165 — Placement compatibility (Cross-cutting).** A Build's placement shall be interpreted
against the applicable canonical geometry and World Space semantics, and an unresolved conflict
caused by Model version or terrain change shall not silently relocate the anchor.

**VIZ-R166 — Build continuity (Visualization).** A loaded Build shall participate in transition,
refinement, occlusion, and embodied presentation coherently without causing the underlying Model
State or canonical environmental observations to change.

**VIZ-R167 — Build availability (Visualization).** Loading, hiding, removal, unsupported content,
or presentation failure shall affect the optional Build manifestation without corrupting its
Impression, its Attractor meaning, or the surrounding Realization.

**VIZ-R168 — Build compatibility and versioning (Cross-cutting).** A Visualization shall identify
whether it can preserve a Build's required authored meaning, and incompatible reinterpretation
shall not be presented as exact reproduction.

## 16. Non-goals

This requirements document intentionally does not choose or define:

- Rust APIs, language-level types, traits, ownership patterns, or crate boundaries;
- ABI or WebAssembly memory layouts, serialization, transport, request/response, or streaming
  methods;
- rendering backend resources, shader contracts, mesh formats, texture formats, scene graphs, or
  draw scheduling;
- user-interface controls, bindings, panels, gestures, camera controls, or screen layout;
- a single map projection, symbol language, artistic style, asset library, animation system, or
  accessibility implementation;
- a fixed world topology, planet generator, spatial partition, level-of-detail algorithm, or
  numerical representation;
- the exact contents of Model State or the metric, topology, and path algorithm for Possibility;
- the gameplay rule coupling Egress to Exploration, the exact use of Resonance, or the resolution
  algorithm for Yearnings;
- networking, accounts, real-time multiplayer, Impression-library services, Attractor abuse
  prevention, moderation, or social identity;
- the authored Build format or content policy; or
- fixed frame-rate, latency, memory, bandwidth, or visual-fidelity targets.

Those choices belong in later contracts, ADRs, designs, and plans. If they implement or constrain
Visualization behavior, their obligations remain traceable to the IDs above and any architectural
decisions should follow the repository's [ADR process](adr/README.md).

## 17. Traceability index

This index is navigational, not a substitute for the full requirements.

| Requirement range | Primary downstream concern |
|---|---|
| `VIZ-R01`–`VIZ-R10` | Responsibility boundaries and traceability |
| `VIZ-R11`–`VIZ-R17` | Capability and semantic sufficiency |
| `VIZ-R18`–`VIZ-R33` | World Space and geometry |
| `VIZ-R34`–`VIZ-R48` | Environmental and semantic fields |
| `VIZ-R49`–`VIZ-R61` | Ecology, species, and organisms |
| `VIZ-R62`–`VIZ-R73` | Canonical observation and inspection |
| `VIZ-R74`–`VIZ-R82` | Top-down map experience |
| `VIZ-R83`–`VIZ-R91` | Embodied and point-of-view experience |
| `VIZ-R92`–`VIZ-R100` | Model time and simulation time |
| `VIZ-R101`–`VIZ-R115` | Egress transition and continuity |
| `VIZ-R116`–`VIZ-R126` | Uncertainty, error, and refinement |
| `VIZ-R127`–`VIZ-R140` | Identity, provenance, and determinism |
| `VIZ-R141`–`VIZ-R152` | Scale and performance semantics |
| `VIZ-R153`–`VIZ-R160` | Compatibility, evolution, and validation |
| `VIZ-R161`–`VIZ-R168` | Builds |
