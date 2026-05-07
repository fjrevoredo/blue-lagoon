# Architecture Diagrams

## Purpose

This document provides the first repository-native diagram set for Blue Lagoon.
It is meant to be useful in two modes:

- as a usage guide for understanding how the runtime behaves
- as a development guide for refactoring and extension work

The diagrams here are intentionally focused. They complement
`docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and
`docs/IMPLEMENTATION_DESIGN.md`; they do not replace them.

## How To Use These Diagrams

- Start with the high-level runtime structure to understand boundaries.
- Use the foreground and background flow diagrams to understand execution.
- Use the governed-action and recovery diagrams before changing control flow.
- Use the persistence map before changing schema, merge paths, or audit logic.

## 1. High-Level Runtime Structure

Use this diagram first when you need to orient yourself in the system.

```mermaid
flowchart TD
    U["Telegram user"]
    TG["runtime telegram"]
    H["runtime harness"]
    CW["conscious worker"]
    UW["unconscious worker"]
    ADM["admin CLI"]
    ACT["governed actions / approvals"]
    DB[("PostgreSQL canonical store")]

    U --> TG
    TG --> H

    ADM --> H
    H --> CW
    H --> UW
    H --> ACT
    H --> DB

    CW --> H
    UW --> H
    ACT --> H
    DB --> H

    UW -. "wake signal proposal" .-> H
    H -. "approved wake trigger" .-> CW
```

Development use:

- If a change crosses one of these arrows, it probably belongs in
  `crates/harness`.
- If a change bypasses the harness, it is probably violating an architectural
  invariant.

## 2. Foreground Request Flow

This is the primary user-facing execution path.

```mermaid
sequenceDiagram
    participant User as Telegram user
    participant Telegram as runtime telegram
    participant Harness as runtime harness
    participant Worker as conscious worker
    participant Store as PostgreSQL canonical store
    participant Tools as tools / external systems

    User->>Telegram: send message
    Telegram->>Harness: ingress event
    Harness->>Store: load identity, memory, state
    Harness->>Worker: trigger + assembled context + budgets
    Worker-->>Harness: plan / action proposals
    Harness->>Harness: policy and budget validation
    Harness->>Tools: approved tool call or external action
    Tools-->>Harness: observation / result
    Harness-->>Worker: observation
    Worker-->>Harness: user reply + episodic record + candidate memory + background request
    Harness->>Store: commit canonical writes
    Harness-->>Telegram: reply payload
    Telegram-->>User: deliver reply
```

Development use:

- Change this flow when working on ingress, context assembly, tool execution,
  episodic recording, or user reply behavior.
- If a feature needs new foreground behavior, decide first whether it belongs
  before worker launch, inside worker reasoning, or in post-worker validation.

## 3. Background Maintenance Flow

This shows the bounded unconscious path.

```mermaid
sequenceDiagram
    participant Trigger as schedule / anomaly / delegation
    participant Harness as runtime harness
    participant Worker as unconscious worker
    participant Store as PostgreSQL canonical store
    participant Foreground as conscious worker

    Trigger->>Harness: background trigger
    Harness->>Store: load scoped inputs
    Harness->>Worker: job kind + scoped inputs + budgets
    Worker->>Worker: analyze and transform
    Worker-->>Harness: memory deltas / index updates / self-model deltas / diagnostics / wake signal
    Harness->>Harness: validate proposals
    Harness->>Store: merge accepted canonical changes
    alt wake signal allowed by policy
        Harness-->>Foreground: approved wake trigger
    else no wake or blocked wake
        Harness->>Harness: record result only
    end
```

Development use:

- Use this before changing background job kinds, proposal merging, or wake
  policy.
- If a background feature wants direct user output or direct writes, it is on
  the wrong side of the architecture boundary.

## 4. Governed Action And Approval Flow

This is the control path that matters most for safe side effects.

```mermaid
sequenceDiagram
    participant Worker as conscious worker
    participant Harness as runtime harness
    participant Policy as policy / capability validation
    participant Approval as approval state
    participant Operator as operator or user
    participant Action as external action
    participant Store as PostgreSQL canonical store

    Worker-->>Harness: governed action proposal
    Harness->>Policy: validate scope, risk, budgets
    alt approval required
        Policy-->>Harness: approval required
        Harness->>Store: persist approval request
        Harness-->>Operator: request approval
        Operator-->>Harness: approve or reject
        Harness->>Store: persist approval decision
        alt approved
            Harness->>Action: execute action
            Action-->>Harness: result
            Harness->>Store: persist execution record
        else rejected
            Harness->>Store: persist blocked outcome
        end
    else approval not required
        Policy-->>Harness: approved for execution
        Harness->>Action: execute action
        Action-->>Harness: result
        Harness->>Store: persist execution record
    end
```

Development use:

- Read this before changing governed action JSON, capability scope checks, risk
  tiers, or approval resolution behavior.
- Side-effecting changes should preserve a clear persisted record of proposal,
  decision, execution, and outcome.

## 5. Recovery Lifecycle

This diagram summarizes the execution lifecycle around interruption and
recovery.

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Running: valid trigger received
    Running --> Completed: finished within budgets
    Running --> AwaitingApproval: approval-gated action
    AwaitingApproval --> Running: approval resolved
    Running --> Interrupted: crash / timeout / lease loss / shutdown
    Interrupted --> Recovering: supervisor recovery event
    Recovering --> Running: checkpoint restored or work resumed
    Recovering --> Failed: recovery not possible
    Failed --> Idle: operator intervention or new trigger
    Completed --> Idle
```

Development use:

- Use this before changing checkpoints, leases, recovery supervision, or resume
  semantics.
- If a new feature introduces long-lived work, it must still fit into this
  bounded lifecycle.

## 6. Canonical Persistence Map

This diagram shows what the harness owns in PostgreSQL and how major runtime
paths relate to canonical state.

```mermaid
flowchart TD
    H["runtime harness"]

    subgraph RuntimeInputs["runtime inputs"]
        I1["ingress events"]
        I2["scheduled foreground tasks"]
        I3["background triggers"]
        I4["approval resolutions"]
    end

    subgraph CanonicalStore["PostgreSQL canonical store"]
        E["episodes / episode messages"]
        M["memory artifacts"]
        S["self-model artifacts"]
        R["retrieval artifacts"]
        A["approval requests"]
        G["governed action executions"]
        W["wake signals"]
        J["background jobs / runs"]
        C["checkpoints / leases / recovery state"]
        D["audit and trace records"]
    end

    I1 --> H
    I2 --> H
    I3 --> H
    I4 --> H

    H --> E
    H --> M
    H --> S
    H --> R
    H --> A
    H --> G
    H --> W
    H --> J
    H --> C
    H --> D
```

Development use:

- Use this before changing migrations, persistence models, merge rules, admin
  surfaces, or traceability behavior.
- If a subsystem writes here directly without going through harness-owned paths,
  it is eroding the canonical write boundary.

## Suggested Next Diagram Work

The next useful diagrams after this first set are:

1. Context assembly internals for the conscious worker
2. Identity and self-model evolution flow
3. Trace explorer causal graph model
4. Admin surface to management-service mapping
5. Background job taxonomy and scheduling map

## Related Documents

- [docs/diagram-strategy.md](docs/diagram-strategy.md)
- [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md)
- [docs/LOOP_ARCHITECTURE.md](docs/LOOP_ARCHITECTURE.md)
- [docs/IMPLEMENTATION_DESIGN.md](docs/IMPLEMENTATION_DESIGN.md)
