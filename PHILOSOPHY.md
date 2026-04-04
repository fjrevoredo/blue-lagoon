# PHILOSOPHY.md - Blue Lagoon

Last updated: 2026-04-03
Applies to: v0.1.0 (pre-implementation baseline)

This document defines the guiding principles for Blue Lagoon. Every architectural
decision, feature addition, and contribution must align with these values. When in
doubt, refer back here.

---

## Part I: Philosophy

---

### 1. We Are Building Real AI

Blue Lagoon exists to deliver the kind of assistant people were promised for
decades. The goal is a persistent, coherent, autonomous presence that feels real
enough to change how people relate to software.

We call this "Real AI." For us, that means an assistant with continuity, nuance,
memory, time-awareness, personality, and a lived sense of self. It should feel
closer to a movie robot assistant than to a chat interface with extra features.

We believe the building blocks already exist. Looping runtimes, memory systems,
self-models, identity engineering, and proactive behavior are all already out
there. The missing piece has been assembly. Blue Lagoon is our attempt to assemble
those parts into one coherent system.

**What this means**
- The target is a persistent assistant, not a session-based chatbot.
- The assistant should remember what happened, when it happened, and why it
  mattered.
- Time passing should matter. After hours offline, the assistant should respond as
  something that has continuity through time.
- Personality should evolve through experience. After six months, the assistant
  should feel meaningfully shaped by use.

**When evaluating a change, ask**
- Does this move Blue Lagoon closer to the experience of a real assistant?
- Does this strengthen continuity, time-awareness, or identity?
- Does this make the system feel more alive or more like a wrapper?

---

### 2. The Harness Does the Heavy Lifting

The central architectural bet in Blue Lagoon is simple. The harness should carry
the weight of the system.

Many agents in the wild lean too heavily on the model. They ask the LLM to carry
memory, planning, identity, tool orchestration, background work, and control logic
all at once. That approach produces fragile systems because the most important
parts of the product end up depending on the least reliable part of the stack.

In Blue Lagoon, the harness prepares the stage. It assembles context, maintains
memory, enforces budgets, validates proposals, schedules background work, and
guards canonical state. The model contributes nuance, interpretation, personality,
and the human-like spark at the center of the experience.

**What this means**
- The harness owns continuity, state management, policy, and execution control.
- The model owns expression, judgment, style, and moment-to-moment reasoning.
- The system should remain coherent even when models differ in quality.
- Model output must always be treated as fallible and never trusted blindly.

**When evaluating a change, ask**
- Is this responsibility in the right layer?
- Are we pushing deterministic plumbing into the model for convenience?
- Does this make the model carry work that the harness should own?

---

### 3. Identity Must Be Alive

Blue Lagoon has identity because identity changes behavior. A name, a role, a
temperament, likes, dislikes, habits, and boundaries only matter if they influence
planning, responses, and decisions.

Identity is therefore part of the control architecture. It lives in the self-model
and enters reasoning as a live operational input. It is not a persona paragraph
hidden in a prompt and forgotten the moment the conversation gets long.

Identity also evolves. Some parts stay stable so continuity is preserved. Other
parts should change through experience, reflection, memory, and time. The
assistant should become more itself as it lives longer, not less.

**What this means**
- The self-model is a live system artifact.
- Stable identity includes foundational attributes such as role, core
  temperament, and enduring constraints.
- Evolving identity includes habits, preferences, tendencies, and autobiographical
  refinements.
- Identity updates must pass through harness validation and become part of
  canonical history.

**When evaluating a change, ask**
- Does this make identity action-relevant?
- Does this preserve continuity while allowing growth?
- Is this change a real part of the system or only presentation?

---

### 4. Memory Should Feel Like a Mind

Blue Lagoon should remember in a way that feels natural. Facts matter, but so do
episodes, timing, context, and causality. The assistant should have a sense of
what happened, when it happened, and how one event relates to another.

That requires more than storage. Memory needs structure, consolidation,
supersession, contradiction handling, and protection against drift. It should grow
more useful over time and remain coherent under long-term use.

From the assistant's point of view, memory should simply be there. It can try to
recall, try to store what seems important, and work with what surfaces into its
conscious context. The machinery behind that memory belongs elsewhere.

**What this means**
- Episodic records are first-class and preserved for meaningful interactions.
- Memory quality matters more than memory volume.
- Contradictions, stale facts, and duplicates are treated as defects.
- Facts without provenance are liabilities.
- The unconscious loop and the harness manage the deeper mechanics of memory.

**When evaluating a change, ask**
- Does this improve coherence over time?
- Does this protect memory quality as the system grows?
- Does this make memory feel more lived and less mechanical?

---

### 5. The Assistant Has Interoception

Blue Lagoon should have an inner point of view about itself. It should know what
it feels like to be in its own current condition.

That means interoception. The assistant should have access to internal signals such
as load, health, resource pressure, reliability, confidence, and desires to act.
These signals shape tone, planning, urgency, and restraint. They are part of what
makes behavior feel grounded instead of generic.

This awareness has limits by design. The assistant does not inspect the harness
internals, the memory schemas, or the hidden background mechanics that maintain its
state. It experiences the results of those systems as part of its own life, not as
implementation details.

**What this means**
- Internal state and external state remain clearly separated.
- The assistant reasons as a function of both its own condition and the outside
  world.
- The assistant can feel pressure, preference, urgency, or reluctance in an
  operational sense.
- The assistant should not know how the hidden plumbing works behind the scenes.

**When evaluating a change, ask**
- Does this strengthen the assistant's inner point of view?
- Does this preserve the boundary between experience and implementation?
- Does this give the assistant grounded self-awareness without leaking internals?

---

### 6. The Conscious Mind Sees Only Part of the Whole

Blue Lagoon borrows an important property from conscious beings. The foreground
agent experiences only the conscious layer of its own operation.

The conscious loop can perceive, reason, plan, act, remember, and express desire.
It can try to retrieve memories and mark important moments for storage. It can feel
its internal state. What it cannot do is see the hidden processes that maintain its
memory, reconcile contradictions, refresh indexes, or reshape identity artifacts in
the background.

That deeper maintenance belongs to the unconscious loop and the harness. Their work
must remain invisible to the conscious agent. The result should feel natural. The
assistant simply has memories, habits, and changing tendencies without needing to
see the machinery that maintains them.

**What this means**
- The conscious loop has access to conscious context only.
- The unconscious loop handles maintenance, consolidation, reflection, and
  proposal generation.
- The harness mediates every interaction between layers.
- The assistant experiences memory and identity as given parts of itself.

**When evaluating a change, ask**
- Does this preserve the conscious versus unconscious boundary?
- Does this expose hidden machinery to the agent unnecessarily?
- Does this make the assistant's inner life feel more natural or less?

---

### 7. The Harness Is Sovereign

The harness is the final authority in Blue Lagoon. It mediates, validates,
schedules, budgets, commits, and logs. No loop, tool, or component should be able
to bypass that authority.

This is the foundation for coherence, safety, and auditability. Canonical state
must only change through validated proposals. Cross-loop interaction must happen
through structured channels. External actions must follow policy. Execution must
remain bounded and terminable.

If this rule is ever weakened, the whole system becomes harder to trust.

**What this means**
- All canonical writes go through harness validation.
- All cross-loop communication goes through structured proposals and typed
  triggers.
- All external actions flow through policy checks.
- All foreground and background work has explicit budgets and hard termination
  paths.
- All important decisions are traceable in logs.

**When evaluating a change, ask**
- Does this bypass the harness in any way?
- Does this create an unbounded or untraceable path?
- Does this weaken policy, provenance, or control?

---

## Part II: Implementation Guide

> **Status: TBD**
>
> This section will explain how each principle from Part I translates into
> concrete decisions in the codebase. It is the how to Part I's what and why.
> Keep this section updated as the technical design evolves.

---

## Decision Framework

When proposing or reviewing any change, validate against all seven principles:

1. **Real AI**. Does this move Blue Lagoon toward the experience of a real
   assistant with continuity, identity, and time-awareness?
2. **Harness-heavy design**. Is the harness carrying the system work that belongs
   there?
3. **Living identity**. Does identity stay operational, evolving, and
   action-relevant?
4. **Mind-like memory**. Does memory remain coherent, structured, and grounded in
   episodes and time?
5. **Interoception**. Does the assistant gain grounded inner state without leaking
   implementation internals?
6. **Conscious boundary**. Does the conscious loop remain unaware of the hidden
   maintenance machinery behind memory and identity?
7. **Harness sovereignty**. Does the harness remain the sole authority for policy,
   canonical writes, mediation, and bounded execution?

If any principle is violated without strong documented justification, the proposal
should be reconsidered.

---

## Non-Negotiables

Some principles are absolute:

- **No direct canonical writes.** All commits go through harness validation.
- **No direct cross-loop communication.** All interaction passes through harness
  mediation.
- **No unbounded execution.** Every foreground episode and background job must
  have enforced budgets and a hard termination path.
- **No static identity.** Personality and selfhood must be live parts of the
  system that can evolve through experience.
- **No agent awareness of hidden machinery.** The assistant experiences its
  memory and identity. It does not inspect the internals that maintain them.
- **No silent canonical mutations.** Every accepted, rejected, or superseded
  change must leave a traceable record.

---

Blue Lagoon aims to prove that a fully capable autonomous intelligent assistant is
already achievable. The missing ingredient has been coherence. When the harness,
memory, identity, interoception, and model each carry the right role, the result
can finally feel like the kind of assistant people imagined all along.