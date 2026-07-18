# New World Model 1

## Purpose

This document defines the conceptual model for an exploration game about continuous travel through
a universe of possible natural worlds. It establishes shared terminology, separates the major
responsibilities of the system, and records the invariants that later design and implementation
plans must preserve.

The model is intended to support many independently evolving Models and Visualizations connected
through well-defined interfaces. It describes the desired experience and the information that must
cross those interfaces without prescribing a particular representation, algorithm, storage format,
or rendering technique.

## Core experience

A Traveler explores a physical world while also moving through a continuum of possible worlds.
There are no portals or discrete reality changes. As the Traveler moves, the world changes gradually
around them. Newly encountered places belong to the Traveler's newer position in Possibility, while
places close to the Traveler preserve enough realization history to make the transition continuous.

The Traveler influences the direction of this change using memories of places, environments,
organisms, and creations they have encountered. The result is intentional but not precisely
predictable: the Traveler expresses what they yearn for, while the Model determines how those
desires can be reconciled in a plausible world.

## Model

A **Model** is a mathematical structure capable of representing limited or idealized natural
planetary geography, environments, and ecosystems. It defines:

- the parameters that describe a world;
- the relationships and constraints among those parameters;
- the spatial structures derived from those parameters;
- which states are valid;
- how nearby or related states can be derived from one another; and
- the model-facing attributes that may be observed and used to guide travel.

A Model is not itself a simulation. It supplies the parameters and relationships from which a
Visualization can construct and simulate an interactive world.

Different Models may represent different kinds of worlds or use different mathematical structures.
The concepts in this document should apply to any Model that can provide the required information
to a compatible Visualization.

### Model state

A **Model State** is a complete assignment of the parameters needed by a Model to specify one
physical world. A single Model State is one point in Possibility.

A Model State describes an entire world, not one local region within a world. Spatial variation in
geography, climate, environments, and ecosystems is part of the world derived from that state.

The compact coordinates used to identify or navigate Model States need not expose every derived
property of a world. A Model may map a compact **Possibility Coordinate** through its constraints and
relationships to a much richer Model State.

## Possibility

**Possibility** is the universe of everything that a Model can represent. Each point in Possibility
corresponds to a unique Model State and therefore to one complete physical world with its own
geography, environments, and ecosystems.

Possibility is intended to feel like a continuum. Neighboring points describe related worlds, with
no discernible boundaries or enumerable collection of separate universes.

### Kinds of Possibility

It is useful to distinguish three related sets:

- **Theoretical Possibility** contains every state permitted by the abstract mathematical Model.
- **Representable Possibility** contains the states that a particular realization of the Model can
  describe, subject to its parameter schema, numeric representation, available generators, and
  other finite constraints.
- **Reachable Possibility** contains the states that can be reached from a given state under the
  Model's movement and plausibility rules.

A state may be theoretically valid but not representable. A representable state may not be
reachable from the Traveler's current state, or may require a long path through intermediate states.

### Movement in Possibility

Movement in Possibility is the derivation of one Model State from another. The difference between
the two states describes a direction and distance in Possibility Space.

Distance in Possibility need not be Euclidean and need not be uniform. Two worlds that differ by a
small numeric amount may have substantially different realized consequences, while larger numeric
changes may be visually subtle. A Model is responsible for defining meaningful relationships,
neighborhoods, constraints, and paths through its Possibility.

The mapping from Model State to a realized world should be stable enough that movement between
nearby points usually produces related worlds. Emergent or chaotic behavior may still create sharp
local differences. Continuity mechanisms at the boundary between successively realized worlds are
therefore an essential part of the experience.

## Realization

A **Realization** is the model-facing description of a physical world derived from a Model State. It
is the output of a Model and the principal input to a Visualization.

A Realization provides the stable, shareable meaning of the world independently of how any
particular Visualization presents it. Depending on the Model, it may describe or make available:

- world geometry and spatial coordinates;
- planetary, stellar, and orbital properties;
- geology, terrain, climate, hydrology, and soils;
- ecological structure, species, niches, and relationships;
- canonical organism and environmental attributes;
- temporal cycles and initial conditions; and
- stable identities or addresses for observable model entities.

A Realization does not prescribe meshes, textures, animation rigs, audiovisual style, interface
layout, or hardware-dependent detail. Those belong to the Visualization.

The boundary between Model and Visualization should allow each to evolve independently to the
greatest practical extent. Compatibility requires an explicit contract describing which
Realization capabilities and attributes a Visualization consumes.

## Visualization

A **Visualization** turns a Realization into an interactive three-dimensional experience. It
deterministically chooses how model entities and processes are presented using parameters that may
include:

- libraries of mesh, texture, animation, sound, and behavior generators;
- artistic direction and presentation rules;
- personal preferences and accessibility choices;
- hardware capabilities and resource limits; and
- deterministic simulation parameters.

Visualization variance is not part of Possibility. Travelers using the same Model can occupy the
same point in Possibility while consuming different Visualizations of it.

Given the same Model and Model State, the same Visualization definition and version, the same
Visualization parameters, the same location, and the same simulation time, the Visualization must
produce the same observable result. Different Visualizations may produce different representations
of the same model entities while preserving their model-defined meaning.

### Simulation

Simulation belongs to the Visualization. It animates and evolves the interactive representation of
a Realization, including such things as organism behavior, weather presentation, environmental
motion, and other transient activity.

Simulation state does not change the Traveler's point in Possibility unless an explicit gameplay
rule converts an observation or action into a request to move through Possibility. A simulation may
add transient detail, but it must preserve the canonical model attributes needed for shared
Impressions and consistent navigation.

### World Space and View Space

**World Space** is the physical coordinate system of a Realization. Locations used for exploration,
Impressions, builds, and travel destinations are expressed in World Space.

**View Space** is the presentation coordinate system used to render or otherwise present a
Visualization. Camera coordinates, screen coordinates, display transformations, and similar
presentation details belong to View Space and are not shareable world addresses.

Movement through the physical environment is movement in World Space. Informally, this may also be
called movement through the Visualization, provided that it is not confused with View Space.

## Traveler

A **Traveler** is the focus of an individual's experience. The Traveler has at least:

- a current point in Possibility;
- a current location in World Space;
- a position in the Visualization's simulation time;
- a collection of Impressions;
- zero or more active Yearnings; and
- presentation preferences associated with the chosen Visualization.

The Traveler moves independently along three conceptual axes:

- **Exploration** changes the Traveler's position in World Space.
- **Egress** changes the Traveler's position in Possibility.
- **Temporal movement** changes the Traveler's position in simulation time.

The experience may couple these axes, but they remain distinct concepts.

## Exploration

**Exploration** is movement through World Space within the physical world described by the current
Realization. It allows the Traveler to encounter geography, environments, ecosystems, organisms,
and creations.

The long-term goal is exploration of complete planets with varied geology and ecosystems, hosting
star and orbiting moon attributes, and diurnal, tidal, and seasonal cycles. A Model may initially
provide a simpler world, such as a mathematically infinite plane whose variation is derived from a
fixed collection of deterministic spatial functions.

Exploration may be slow and intimate or fast enough to survey large landscapes. The Traveler may
also accelerate, slow, pause, or possibly reverse supported temporal cycles without necessarily
moving through Possibility.

## Egress

**Egress** is movement through Possibility. It changes the fundamental Model State that shapes the
physical world in which Exploration takes place.

Egress is conceptually independent of Exploration, but the game requires simultaneous movement in
World Space. A Traveler cannot Egress while stationary. This coupling is a rule of the Traveler and
gameplay experience, not a responsibility of the Model or Visualization.

As Egress proceeds:

- the Traveler follows a continuous path through nearby reachable Model States;
- successive worlds differ gradually whenever the Model permits it;
- newly encountered regions are realized according to the Traveler's newer Model State; and
- previously encountered or nearby regions retain sufficient realization history to conceal
  discontinuities between successive worlds.

The visible environment is therefore a continuity-preserving presentation of the Traveler's path
through complete worlds. Regional history does not imply that different regions define the
Traveler's current point in Possibility.

### Continuity

The transition between worlds should never appear as a global reload. A Visualization may preserve,
blend, or reconcile content around the boundary between regions realized from successive Model
States. It may use different techniques for terrain, atmosphere, vegetation, organisms, and other
content.

Continuity has two complementary sources:

1. The Model should make nearby points in Possibility produce related Realizations where practical.
2. The Visualization should mask unavoidable discontinuities caused by emergence, chaotic
   behavior, finite resolution, or changes in presentation.

Continuity techniques must not change the Traveler's canonical Possibility or World Space address.
They are a presentation of the path already traveled.

### Egress capability and resonance

Some Model States, directions, or environments may offer few plausible continuations satisfying a
Traveler's active desires. The game may represent the ability to Egress through a local signal
called **Resonance**.

Resonance is a gameplay property of the Traveler's interaction with a Model and its current
Realization. It is not a property of a particular Visualization: choosing different visuals or
hardware must not change Reachable Possibility.

Resonance may limit or slow Egress when the requested direction has too few nearby solutions. High
Resonance indicates that the Traveler has strong support for movement in the desired direction;
low Resonance indicates ambiguity, incompatibility, or insufficient local support.

The exact role of Resonance remains an open design question. Possibilities include requiring a
minimum Resonance to begin or continue Egress, scaling the rate or precision of Egress, or using
Resonance as a measure of confidence in the requested direction. Any rule must preserve the
principle that Visualization choice does not change Reachable Possibility.

## Organisms

A Model defines species, ecological relationships, and canonical organism attributes. A
Visualization populates World Space with individual **Organisms** representative of those species
and simulates their behavior within the ecosystem.

An organism therefore has two related descriptions:

- its **model manifestation**, containing canonical species identity and observable model
  attributes; and
- its **visual manifestation**, containing the meshes, textures, animations, sounds, and transient
  simulation state chosen by the Visualization.

Two compatible Visualizations may depict the same model manifestation differently. An Impression
of the organism must retain the same model meaning across those Visualizations. Travelers using an
identical Visualization configuration at the same location and simulation time should observe the
same visual manifestation.

## Impressions

An **Impression** is a durable memory and address created by a Traveler. It captures:

- the Traveler's exact point in Possibility;
- the Traveler's exact location in World Space;
- sufficient Model and Visualization identity to interpret the address;
- the relevant simulation time or temporal phase when required; and
- optionally, the canonical attributes of an organism, environment, feature, or other subject at
  that location.

An Impression records an observation. It does not by itself steer Egress. The Traveler chooses how
to interpret one or more Impressions by placing them in Yearnings.

Impressions may be private, shared directly, or published to a shared Impression Library. A
Traveler with a compatible Impression can visit the same point in Possibility and the same location
in World Space. The shared guarantee is the same Realization and canonical model subject, not an
identical presentation under different Visualizations.

An Impression may also carry a Build. Loading or removing such an Impression controls whether its
Build appears in the Traveler's Visualization.

## Yearnings

A **Yearning** is a configured set of Impressions that influences the direction of Egress. A
Traveler may activate multiple Yearnings simultaneously.

Each Yearning has:

- one or more source Impressions;
- a weight determining its contribution relative to other active Yearnings;
- an Influence configuration for individual observed attributes; and
- a Scope describing how prevalent selected attributes should become.

### Influence

For each usable attribute of an Impression, a Traveler may choose one of four intentions:

- **Accentuate** asks for the attribute to become more strongly represented.
- **Repress** asks for the attribute to become less strongly represented.
- **Hold** asks for the corresponding aspect of the current Model State to resist change.
- **Disable** causes the attribute to make no contribution to the Yearning.

Hold and Disable are distinct. Disable expresses no preference. Hold is an active constraint that
competes with other requested changes.

### Scope

Scope describes the desired prevalence of selected attributes in a destination world. It ranges
continuously or through named bands from **singular**, through **common**, to **pervasive**.

For organism attributes:

- singular asks for one species, or a small exceptional lineage, to express the attributes;
- common asks for the attributes to occur among multiple species; and
- pervasive asks for the attributes to characterize most applicable species.

Scope applies to species and ecological distributions, not to one captured individual. For
environmental attributes, Scope similarly describes how frequently or extensively the attribute
should occur throughout the destination world.

Scope is statistical, not spatial influence around the Impression's World Space location.

### Resolving Yearnings

Active Yearnings jointly describe an intent rather than an exact destination. The Model reconciles
their weighted, potentially conflicting requests with:

- the current Model State;
- held attributes;
- Model constraints and relationships;
- the local structure of Possibility; and
- the maximum permitted movement during the current Egress step.

The result must be independent of the order in which Yearnings or Impressions are evaluated.
Weights express relative compromise, not processing priority. Model validity takes precedence over
literal satisfaction of a Yearning.

This process should permit surprising consequences. A Traveler can exert fine-grained influence
over direction without predicting the exact geography, ecosystem, or organisms that will emerge.

## Attractors

An **Attractor** is a historically accumulated indication that Travelers have visited or published
Impressions near a region of Possibility. Attractors let a Traveler sense and move toward community
activity without first finding an exact Impression in the shared library.

An Attractor may be diffuse or precise. A weak Attractor formed from a small number of related
visits may indicate only a broad region and approximate direction in Possibility. As evidence
accumulates, the Attractor may resolve toward a precise destination resembling an Impression.

Attractor strength may depend on:

- the number and distribution of qualifying visits;
- published Impressions at or near the destination;
- repeat visits by independent Travelers;
- Builds associated with published Impressions; and
- personal relevance, such as subscriptions to selected creators' Impressions.

Attractors are historical rather than real-time multiplayer presence. They may also be constrained
to a time-bounded community expedition lasting days or weeks. This does not preclude a future
real-time multi-Traveler experience, but such an experience is not required by the Attractor model.

### Visit reporting

A Traveler may choose whether anonymous visits contribute to Attractors. Reporting defaults to
enabled. Reports are not associated with a public user identity or personal profile.

The service may nevertheless require a private persistent identity or equivalent proof so it can
rate-limit contributions, count independent visitors, and resist fabricated popularity. Such an
identity is an abuse-prevention mechanism, not a social identity exposed through the game.

Reported coordinates must identify a representable and reachable destination under the referenced
Model. The service may validate this property before accepting a contribution.

Published Impressions can be removed by their creator or through community moderation. Removing
them also removes their contribution to Attractor strength. Attractors do not otherwise need to
decay merely because time has passed.

### Dual-space travel

An Attractor may describe both a region of Possibility and a desired location in World Space, but
World Space locations are meaningful only within their associated Model State. Possibility distance
and World Space distance are therefore separate.

A Traveler moving toward an Attractor may:

- Egress toward its region in Possibility;
- Explore toward its World Space location; or
- do both simultaneously.

The Traveler may arrive along either axis first. The experience should, where practical, coordinate
Exploration and Egress so arrival in Possibility and World Space occurs at approximately the same
time. Coordination must adjust rates or paths without treating the two spaces as one metric.

Whether a Traveler can arrive at an Attractor exactly depends on its precision and strength. A
diffuse Attractor can provide only a bias or approximate direction. A sufficiently strong and
well-resolved Attractor may expose an exact destination equivalent to an Impression.

## Builds

A **Build** is a Traveler-created monument, artwork, terrain modification, or modular construction
associated with an Impression. Build data is published with that Impression to a shared service.

Builds do not alter the Model State or Possibility. They are optional additions to a Traveler's
Visualization of a Realization. A Build appears only when its Impression is loaded, and a Traveler
may remove or hide it from their Visualization.

Because a Build is anchored to both a point in Possibility and a World Space location, it can be
reconstructed at the intended world and place. Compatible Visualizations should preserve the
Build's authored structure and meaning, though presentation may vary where the Build format permits
it.

Published Builds strengthen Attractors at their locations. This allows monuments and art to become
discoverable through community travel as well as through direct browsing or sharing of their
Impressions.

Build moderation, content policy, and the social identity of creators are separate design areas.
They do not change the world model defined here.

## Determinism and identity

The Model and Visualization are independently deterministic.

For a Model, the same Model definition and version, Model State, World Space location, and relevant
model time produce the same Realization and canonical model entities.

For a Visualization, the same Visualization definition and version, Realization, Visualization
parameters, World Space location, and simulation time produce the same observable presentation and
simulation state.

This separation permits personal or hardware-dependent presentation while retaining shared
meaning. An Impression identifies the Model and canonical location first; it may additionally
identify the Visualization configuration needed to reproduce a particular presentation exactly.

Changes that alter either deterministic mapping require distinguishable version identity so an
Impression is never silently interpreted as a different world or presentation.

## Pluggability and compatibility

Models and Visualizations should be pluggable and capable of independent evolution. Their contract
must allow a participant to determine whether a given pair is compatible.

At a conceptual level, a compatible Model must provide:

- addressable Model States and World Space;
- deterministic Realizations;
- canonical observable attributes and identities;
- relationships, constraints, and reachability through Possibility;
- interpretation of Yearning intent; and
- enough continuity information for transitions between nearby states.

A compatible Visualization must provide:

- deterministic presentation of the required Realization capabilities;
- an interactive World Space experience;
- deterministic simulation and temporal addressing;
- preservation of canonical model observations used by Impressions;
- continuity presentation between successively realized worlds; and
- optional presentation of Builds without changing Model State.

Compatibility may be partial. A Visualization may decline to present a Model whose required
capabilities it does not understand. A Model or Visualization update must declare whether existing
Impressions remain meaningful and reproducible.

## Conceptual invariants

Later design and implementation work should preserve these invariants:

1. One point in Possibility represents one complete physical world.
2. The Traveler has one canonical current point in Possibility, even while nearby content retains
   realization history for continuity.
3. Possibility and World Space are independent spaces with independent notions of distance.
4. Egress is movement in Possibility; Exploration is movement in World Space.
5. Gameplay requires Egress to be accompanied by Exploration, but neither the Model nor the
   Visualization owns that coupling.
6. A Realization carries stable model meaning across compatible Visualizations.
7. Simulation belongs to the Visualization and does not redefine Possibility.
8. Identical Model inputs reproduce the same Realization; identical Visualization inputs reproduce
   the same presentation and simulation state.
9. Impression addresses and observed model attributes remain meaningful across compatible
   Visualizations.
10. Yearnings express weighted intent; the Model resolves that intent without depending on input
    order.
11. Scope describes prevalence in a destination world, not physical falloff around a location.
12. Visualization choice and hardware capability do not change Reachable Possibility.
13. Builds are optional Visualization content associated with Impressions and do not alter the
    Model State.
14. Attractor evidence is historical, abuse-resistant, and removable when its contributing
    published records are removed.

## Open design questions

The following questions remain intentionally unresolved:

- What information constitutes a Model State, and what compact Possibility Coordinate identifies
  or navigates it?
- What metric, neighborhood relation, or topology best describes movement through Possibility?
- How does a Model expose continuity risk, sensitivity, or chaotic divergence between nearby
  states?
- What is the exact Realization contract, and how are required versus optional capabilities
  negotiated?
- Which temporal properties belong to canonical model time, and which belong only to Visualization
  simulation time?
- What canonical organism information is sufficient for an Impression across different
  Visualizations?
- How are attribute-level Yearning requests and prevalence Scope represented consistently across
  different Models?
- How strongly may Hold resist changes required by model validity or by higher-weight Yearnings?
- Is Resonance a threshold, a rate limit, a precision signal, or some combination of these?
- How should Exploration and Egress be coordinated for simultaneous arrival at a dual-space
  destination?
- When does a diffuse Attractor become precise enough to expose an exact destination?
- What anonymous proof and rate limits provide adequate Attractor abuse resistance without creating
  a public user identity?
- Which parts of a Build must reproduce identically across Visualizations, and which may be
  reinterpreted stylistically?
