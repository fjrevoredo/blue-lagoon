# Internal Documentation

## Purpose

Developer reference for implementing, debugging, and extending the harness. Bridges the gap between the architecture docs (the *why*) and the running code (the *what*).

## Audience

Engineers and Claude Code sessions working on this codebase. Assumes familiarity with `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`.

## Relationship to Canonical Docs

Internal docs must never contradict canonical docs. If a conflict is found, the canonical doc wins and the internal doc must be updated. Do not duplicate architecture rationale here — link to the canonical doc instead.

---

## Folder Structure

```
docs/internal/
├── INTERNAL_DOCUMENTATION.md      ← this file (meta-doc, template, conventions)
├── conscious_loop/
│   ├── CONTEXT_ASSEMBLY.md         ← pipeline, limits, message layering, token budget
│   └── GOVERNED_ACTIONS.md         ← JSON schema, capability_scope, validation, risk tiers
└── harness/
    ├── TRACE_EXPLORER.md           ← trace CLI, model-call records, causal links
    └── TOOL_IMPLEMENTATION.md      ← E2E guide for architecture-compliant tools
```

Planned additions (not yet written):
- `unconscious_loop/BACKGROUND_JOBS.md`
- `self_model/SELF_MODEL_EVOLUTION.md`
- `harness/POLICY.md`
- `harness/RETRIEVAL.md`

---

## Document Template

Every document in `docs/internal/` must follow this four-part structure. Section titles must match exactly so they remain stable when rendered as a webpage.

---

### 1. Overview

*What is this subsystem and why does it exist?*

One or two paragraphs. Describe the responsibility of the subsystem, what problem it solves, and where it sits in the overall runtime. Assume the reader has read the canonical architecture docs but has not read the code.

Do not describe *how* it is implemented here — that belongs in section 2.

---

### 2. Implementation

*How is it built?*

Open with a **Source Files** table listing every file that owns the behavior described in this document:

| File | Relevant symbol |
|---|---|
| `path/to/file.rs` | `function_name()` (line N) |

Then cover the key structures, data flow, and algorithms. Use sub-sections as needed. Code references use `path/to/file.rs:line_number` format.

Rules:
- Validation rules and defaults are expressed as tables or bulleted lists, not prose.
- Unimplemented stubs are marked with a `> **NOT IMPLEMENTED:**` callout block.
- Do not duplicate information already in the canonical docs — link to them instead.

---

### 3. Configuration & Extension

*How do operators tune defaults, and what are the extension points?*

List every operator-configurable knob that affects this subsystem — config file keys, environment variables, seed files. For each, state the default, the valid range, and where in code it is read.

Then describe the intended extension points: where to add a new variant, what interface to implement, what test suite to run.

---

### 4. Further Reading

*Where to go next.*

Bulleted list of related documents — other internal docs, canonical docs, or relevant source files. One sentence per link explaining what it adds.

---

## Document Conventions

- Documents are dated with the last commit and session in which they were verified (bottom of file).
- When code moves or is renamed, update the line references before the next verified-date stamp.
- Do not add sections outside the four-part template without first updating this meta-doc.
