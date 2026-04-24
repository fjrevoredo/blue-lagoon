# Blue Lagoon

## Phase 7 Detailed Implementation Plan

Date: 2026-04-24
Status: Planned
Scope: High-level plan Phase 7 only
Audience: LLM-assisted implementation work and human review

## Purpose

This document defines the detailed implementation plan for Phase 7 of Blue
Lagoon.

It exists to close remaining implementation drift after Phase 6 without
weakening the canonical requirements, loop architecture, or implementation
design. Phase 7 also carries the deferred user-facing documentation work that
should be written only after the shipped runtime behavior matches the canonical
product surface.

## Canonical inputs

This plan is subordinate to the following canonical documents:

- `PHILOSOPHY.md`
- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`
- `docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md`
- `docs/PHASE_6_DETAILED_IMPLEMENTATION_PLAN.md`

If this document conflicts with the canonical documents, the canonical
documents win.

## Planning posture

Phase 7 is not a justification for changing requirements to match the code.
Its role is the opposite:

- canonical requirements remain unchanged unless there is an explicit separate
  design decision
- any discovered mismatch between implementation and canonical design must be
  resolved by implementation work or by explicit deferral in planning docs
- user-facing documentation must describe shipped workflows, not aspirational
  or inferred behavior

## Phase 7 focus areas

### 1. Scheduled foreground task support

The canonical documents still require scheduled foreground tasks as part of the
conscious trigger model. The implementation work in this phase must add that
capability end to end.

Required implementation areas:

- trigger contracts and payload shapes for scheduled foreground work
- harness-owned scheduling, persistence, deduplication, and auditability
- policy gates and budget assignment for proactive foreground execution
- recovery handling for interrupted scheduled foreground executions
- operator inspection and bounded explicit control where the workflow belongs in
  the management CLI
- automated tests at unit, component, and integration layers

### 2. Post-Phase-6 drift audit

After scheduled foreground work is implemented, the repository needs another
full consistency pass against the canonical documents.

Required audit outputs:

- confirmation that `docs/REQUIREMENTS.md`,
  `docs/LOOP_ARCHITECTURE.md`, and
  `docs/IMPLEMENTATION_DESIGN.md` match the shipped behavior
- confirmation that the high-level roadmap and detailed plans reflect the real
  current state
- explicit documentation of any newly discovered mismatch before further phase
  execution continues

### 3. User-facing documentation

Once the required runtime surface is actually shipped, Phase 7 should publish
the user-facing docs that were deferred during the drift closure.

Required documentation outputs:

- a true user-facing `README.md` with setup and common usage instructions
- a dedicated user manual for normal workflows, approvals, recovery,
  troubleshooting, and upgrades
- consistency checks to ensure user-facing docs do not conflict with the
  canonical architecture and requirements docs

## Exit criteria

Phase 7 is complete only when all of the following are true:

- scheduled foreground task support exists in the shipped runtime and is
  covered by automated tests
- the canonical requirements and architecture docs no longer need caveats to
  fit the implementation
- user-facing documentation describes real supported workflows rather than
  planned behavior
- no known post-Phase-6 drift remains undocumented
