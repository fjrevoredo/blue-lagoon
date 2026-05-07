# Diagram Strategy

## Purpose

This document defines how Blue Lagoon uses diagrams in repository
documentation.

Diagrams exist to support two jobs:

- explain how the runtime works
- make refactoring and extension boundaries easier to reason about

## Standard

Blue Lagoon uses **Mermaid embedded in Markdown** as the default diagram
format for repository documentation.

Diagram source lives in Markdown files. Exported image artifacts are optional
and must not become the source of truth.

## Scope

Use Mermaid diagrams for:

- architecture overviews
- request and maintenance flows
- governed-action and approval flows
- recovery and lifecycle views
- persistence and subsystem relationship maps

The current repository diagram set lives in
`docs/architecture-diagrams.md`.

## Principles

### 1. Keep diagrams close to the explanation

Put the diagram in the same Markdown document as the text that explains it, or
link directly to a focused diagram document when one shared diagram set is more
useful.

### 2. Prefer focused diagrams

Use several small diagrams with one clear job each. Do not maintain a single
"everything" diagram for the whole system.

### 3. Use canonical repository vocabulary

Diagram labels should match the canonical architecture documents. Prefer terms
such as:

- conscious loop
- unconscious loop
- harness
- canonical store
- governed action
- approval request
- wake signal

### 4. Preserve architecture boundaries

Diagrams must reflect the repository's fixed architectural posture:

- the harness is the sole mediator between loops
- the harness is the sole canonical writer
- foreground and background execution remain isolated
- policy, validation, budgeting, and recovery are harness-owned concerns

### 5. Treat diagrams like code

When a change modifies a documented runtime flow, lifecycle, or responsibility
boundary, update the relevant diagram in the same change set.

## Syntax Baseline

Prefer stable Mermaid syntax that renders cleanly in GitHub Markdown:

- `flowchart`
- `sequenceDiagram`
- `stateDiagram-v2`

Use newer Mermaid features only when they materially improve clarity and have
been verified to render correctly in the repository's normal review surfaces.

## Ownership

Canonical behavior still belongs in:

- `docs/REQUIREMENTS.md`
- `docs/LOOP_ARCHITECTURE.md`
- `docs/IMPLEMENTATION_DESIGN.md`

Diagrams are supporting documentation. They must stay aligned with those
documents and must not introduce conflicting behavior definitions.

## Initial Diagram Set

The baseline diagram set for this repository is:

1. High-level runtime structure
2. Foreground request flow
3. Background maintenance flow
4. Governed action and approval flow
5. Recovery lifecycle
6. Canonical persistence map

## Change Rule

If a code or documentation change affects one of those areas, check whether
`docs/architecture-diagrams.md` also needs to change before merge.
