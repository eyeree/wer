

(The) Model - A mathematical structure capable of representing limited/idealized natural planetary
geography, environments, and ecosystems. The Model is not a simulation, it provides the parameters
that can be feed into such a simulation and defines how those parameters relate to each other.

(The) Possibility - The universe of everything that can be represented by the model. Each
point in The Possibility (a specific set of values for the parameters defined by The Model)
represents an unique physical world with it's own geography, environments, and ecosystems. There is
a Theoretical Possibility, which mathematical in nature, and an Implemented Possibility, which is
constrained by implementation details such as floating point size. 

Possibility Space - When we say "movement in Possibility Space" (or "movement in The Possibility",
or just "movement in Possibility") we mean the derivation of a set of model parameters related in
some way to another set of model parameters (the difference being the "direction" of movement in
Possibility Space).

(The) Visualization - Responsible for turning a model state into an interactive 3d environment. Many
different representations of a given point in The Possibility may be possible, and the Visualization
deterministically determines the representation that is used based on a set of parameters, which
include a library of mesh, texture, animation, and behavior generators along with personal
preferences, and which may be limited to match hardware capabilities. The Visualization adds a whole
other dimension of variance to the system, but this dimension is not considered part of The
Possibility. The Possibility is the same for everyone (with the same model implementation), while
The Visualization may vary for each individual consuming The Visualization.

Visualization Space - The physical 3d world created by The Visualization to represent a point in
Possibility Space. When we say "movement in Visualization Space" (or "movement in The
Visualization", or just "movement in Visualization") we mean movement in this 3d world.

(The) Traveler - A focus point (model state) in The Possibility that represents the individual
consuming The Visualization. The Traveler can move through The Possibility in two different ways:
Exploring and Yearning.

Exploration - Allows the Traveler to move in Visualization Space. The long term goal is be to
represent an entire planet (with hosting star attributes and orbiting moon attributes) with varied
geology and ecosystems; along with diurnal, tidal, and seasonal cycles. Initially it will be limited
to a (mathematically) infinite plane with variation derived from a fixed set of deterministic noise
functions, while Egress changes the parameters of those noise functions. While Exploring, the
Traveler can also speed up (and maybe reverse) temporal cycles.

Egress - Allows the Traveler to move through Possibility Space. Egress changes the fundamental
properties that shape the physical 3d world where Exploring takes place. In game, Egress requires
simultaneous Exploration (movement in Visualization Space), but this isn't required by The Model and
is managed by The Visualization. The direction of Egress (direction of movement in the Possibility
Space) is determined by Yearnings.

Organism - The Visualization populates the Visualization Space species of flora and fauna as
directed by a position in Possibility and Visualization Space. It generates individual Organisms as
representative of these species, and simulates the behavior of these Organisms within their
ecosystems.

Memory - A Traveler can create a Memory to capture their current coordinates in The Possibility and
The Visualization. A Memory may also include the attributes of an individual Organism at that
location in The Visualization. Memories can be shared with other Travelers, which allows that
Traveler to visit that same location in both Possibility and Visualization Space.

Yearning - A set of memories which influences the direction of Egress. A Traveler may have multiple
Yearnings active at the same time. Each Yearning has a weight that determines it's contribution to
the overall direction of Egress. The Traveler can adjust the Influence that the individual
attributes of a Memory in a Yearning has on Egress. This allows the Traveler to cause the aspect of
The Possibility represented by the attribute to be accentuated, repressed, or held steady during
Egress. They can also disable an attribute, indicating the Yearning has no influence on that aspect
of The Possibility. The Traveler can also set the Scope of an Yearning, which varies from
"singular", through "common", to "pervasive". For Memories with Organisms, this applies to species
not individuals (i.e. a single species with the selected trait(s) vs many species with the selected
traits, to most species with the selected traits). When applied to environmental components
(geography, etc.) it similarly determines how frequently the attribute is to appear at a destination
point in Possibility Space. By grouping various Memories into multiple Yearnings and adjusting the
Influence of the Memories and the Scope of the Yearning, and the relative weights of the Yearnings,
the Traveler can exert fine grained control over the direction of Egress, even though the exact
nature of the changes they see will be extremely difficult to predict.

Attractors - A service could track the points visited by Travelers in Possibility and Visualization
Space, grouping nearby points to determine a set of Attractors, effectively a possibly diffuse
region of Possibility and Visualization Space. A Traveler can "sense" Attractors in Possibility
and/or Visualization Space and choose to travel "toward" those regions, as a alternative for
following their Yearnings. The distance of the Traveler form an Attractor and the strength of the
Attractor can influence the Traveler's ability to sense, and move precisely toward, the Attractor.

Notes:

The Possibility vs. The Multiverse - Mostly the same concept, but people usually think of the
multiverse as containing well defined slices of reality. With The Possibility, we are trying to
evoke the feeling of a continuum with no discernable boundaries.

