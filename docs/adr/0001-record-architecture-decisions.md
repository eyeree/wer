# 1. Record architecture decisions

Date: 2026-07-10

## Status

Accepted

## Context

The implementation plan (`implementation-plan.md`) is explicitly phased and calls
for architecture decision records as a Phase 0 deliverable. Decisions made early
about determinism, crate boundaries, and portability constrain every later
subsystem, so we need a durable, reviewable record of *why* each was made.

## Decision

We will keep Architecture Decision Records in `docs/adr/`, one Markdown file per
decision, numbered sequentially, using Michael Nygard's template. Records are
immutable once accepted; a later decision that changes course is a new ADR that
supersedes the earlier one.

## Consequences

- Rationale survives even as code and contributors change.
- Reviewers can challenge a decision by proposing a superseding ADR.
- The index in `docs/adr/README.md` must be updated when an ADR is added.
