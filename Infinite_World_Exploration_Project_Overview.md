# Project Overview: Infinite World Exploration Game

## High Concept

An exploration game inspired by the fantasy of walking between adjacent
realities (similar in spirit to *The Chronicles of Amber*), but without
explicit portals or loading screens.

The player continuously moves through a **continuum of possible
worlds**. As they travel, the current world gradually transforms into
neighboring possibilities. The transformation is seamless and immersive
rather than discrete.

The emphasis is on discovery, beauty, ecology, creativity, and social
exploration rather than combat or survival.

------------------------------------------------------------------------

# Core Experience

Players explore dense, procedurally generated natural environments
filled with rich flora and fauna.

Every journey subtly changes the world.

The player intentionally guides those changes using
**anchors**---captured characteristics of organisms, landscapes,
weather, geological formations, or other discoveries.

Movement is simultaneously through:

-   physical space (moving across terrain)
-   possibility space (changing which nearby world is being realized)

------------------------------------------------------------------------

# Design Goals

-   Continuous transitions between worlds.
-   Dense, believable ecosystems.
-   Grand scenic vistas.
-   Strong feeling of immersion.
-   Emphasis on curiosity instead of combat.
-   Social discovery and collaborative exploration.
-   Infinite replayability through procedural generation.

------------------------------------------------------------------------

# Anchors

Players collect anchors primarily by photographing interesting
discoveries.

Examples:

-   plant
-   animal
-   rock formation
-   cloud type
-   river
-   landscape
-   atmospheric phenomenon

Each anchor captures part of the underlying procedural "genetics" rather
than simply storing an asset.

Players choose which categories of traits become active.

Possible categories include:

-   morphology
-   coloration
-   scale
-   branching patterns
-   ecological traits
-   behavior
-   climate affinity

Anchors can be:

-   emphasized
-   neutral
-   suppressed (anti-anchors)

The active anchor set continuously biases future world generation.

------------------------------------------------------------------------

# Procedural Genetics

Every organism is generated from procedural genomes.

Separate genomes may exist for:

-   appearance
-   behavior
-   ecological niche

World generation proceeds hierarchically:

1.  climate
2.  geology
3.  hydrology
4.  soils
5.  vegetation
6.  food web
7.  organisms
8.  local variation

The objective is ecological plausibility rather than scientific
simulation.

------------------------------------------------------------------------

# Continuous World Transformation

The world should never visibly "reload."

Instead:

-   nearby areas remain stable
-   distant regions gradually converge toward new ecological states
-   atmosphere, lighting, weather, vegetation, and wildlife evolve
    continuously

Large-scale changes should appear first in:

-   sky
-   haze
-   color
-   canopy
-   weather
-   distant ecology

Fine detail resolves as the player approaches.

------------------------------------------------------------------------

# Rendering Strategy

Avoid morphing every object.

Instead, treat the world as procedural fields sampled at multiple
resolutions.

Near field:

-   full geometry
-   animation
-   interaction

Mid distance:

-   clustered procedural vegetation
-   simplified organisms

Far distance:

-   terrain
-   canopy maps
-   shader-driven ecology
-   atmospheric effects

This allows expansive vistas while maintaining continuous transitions.

------------------------------------------------------------------------

# Movement

Two complementary movement modes are envisioned.

## Local Movement

Fast traversal for exploration.

Could include:

-   hovering
-   gliding
-   rapid movement

Used to survey terrain and reach interesting locations.

## Reality Transition Movement

Slow, deliberate movement that changes the world.

Only this mode significantly alters the current world.

This preserves immersion while keeping world evolution manageable.

------------------------------------------------------------------------

# Player Avatar

Current favorite concept:

The player controls a hovering plasma orb or extradimensional explorer.

Advantages:

-   avoids survival constraints
-   simplifies traversal
-   allows gliding
-   visually distinctive

While transitioning between realities, the orb emits arcs of energy that
connect with nearby plants, rocks, and terrain.

Conceptually the orb resonates with nearby reality rather than moving
through a separate dimension.

Dense ecosystems provide many connection points.

Sparse environments make transition difficult or impossible.

------------------------------------------------------------------------

# Social Features

Players leave persistent "paths" through possibility space.

Frequently used routes become easier to follow.

Players may:

-   share anchors
-   publish expeditions
-   exchange routes
-   collaborate on discovering new worlds

Different players may intentionally steer worlds in complementary
directions.

------------------------------------------------------------------------

# Community Atlas

Long-term goal:

A shared atlas documenting:

-   species
-   ecosystems
-   landscapes
-   migration paths
-   photographs
-   expedition journals

Players become explorers and naturalists.

------------------------------------------------------------------------

# Gameplay Direction

Avoid traditional survival loops as the primary gameplay.

Potential activities:

-   discovering rare ecosystems
-   ecological puzzles
-   documenting species
-   photography
-   collaborative exploration
-   cultivating beautiful worlds
-   following famous expeditions

The core reward is discovery.

------------------------------------------------------------------------

# Technical Philosophy

Do not simulate the entire world.

Instead:

-   generate deterministic procedural worlds
-   simulate only nearby activity
-   fake long-term ecological history where needed
-   prioritize internal consistency over scientific accuracy

------------------------------------------------------------------------

# Possible Terminology

Avoid using "Shadow."

Internal development term:

-   possibility space

Potential in-world concepts:

-   resonance
-   drift
-   weave
-   bloom
-   fold
-   verge

Another option is to leave the underlying phenomenon unnamed and instead
speak of "the world drifting" or "pulling reality."

------------------------------------------------------------------------

# Monetization Principles

Do **not** gate exploration or creativity.

Potential monetization:

-   persistent preserves
-   hosted shared worlds
-   collaborative world hosting
-   journals and museum tools
-   cosmetic customization
-   AI-assisted field guides and expedition books

Monetization should support stewardship of a shared multiverse rather
than selling access to discoveries.

------------------------------------------------------------------------

# Core Vision Statement

The player does not travel across an infinite collection of disconnected
procedural worlds.

Instead, they experience **one continuous journey through an infinite
landscape of possible worlds**, intentionally steering reality by
carrying forward the characteristics of the living things and places
they choose to remember.
