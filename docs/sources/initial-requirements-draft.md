
# Blue Lagoon - Initial Requirements Draft

    Blue Lagoon is envisioned as a persistent, proactive, always-on personal AI assistant rather than a task-triggered agent or a conventional chat interface. It is intended to operate as a true looping runtime that remains available over time, responds to user interaction, performs approved proactive behaviors, and maintains continuity across sessions through durable memory and self-modeling.[^1][^2]
    
    The system is based on a dual-loop architecture composed of a conscious loop and an unconscious loop, with a central harness acting as the sole mediator between loops, tools, memory, policies, and canonical storage. The conscious loop is responsible for foreground cognition, user interaction, planning, decision-making, and explicit action. The unconscious loop is responsible for bounded background maintenance tasks such as consolidation, summarization, reflection, contradiction detection, retrieval maintenance, and controlled self-model updates.[^1]
    
    The two loops must remain isolated at the process, context, and writable-data levels. No direct calls, direct wake-ups, or shared writable memory are permitted between them. All communication must occur through structured proposals, typed triggers, and harness-controlled merges into canonical stores.[^1]
    
    ## Core operating model
    
    Blue Lagoon should function as a long-running personal assistant process that can be triggered by direct user input, scheduled foreground tasks, approved wake signals from background processing, supervisor recovery events, and approval resolution events. This places it in the class of always-on persistent daemons rather than one-shot autonomous task runners.[^2][^1]
    
    The conscious loop should build each active context from a compact self-model snapshot, selected relevant memory, and the current internal system state. It should then perceive, plan, propose actions, invoke tools only through the harness, observe results, emit structured episode records, and halt when goals are satisfied or budgets are exhausted.[^1]
    
    The unconscious loop should run only as harness-managed background jobs with explicit scoping, explicit budgets, and no persistent worker identity. Its role is not to act outwardly, but to analyze, consolidate, transform, detect drift, generate structured proposals, and optionally raise typed wake signals for the harness to evaluate under policy.[^1]
    
    ## Self-model and identity
    
    Blue Lagoon shall maintain an explicit and persistent self-model that represents â€œwho I am right nowâ€ in operational terms, not merely stylistic terms. This self-model must include identity, internal state, active goals, constraints, preferences, and other control-relevant properties, and it must be passed in compact form into foreground reasoning so that outputs remain grounded in the assistantâ€™s own continuity and condition.[^3][^1]
    
    The initial identity profile shall support concrete and character-like attributes, including at minimum:
    
    - Name.[^3]
    - Species.[^3]
    - Core role or archetype.[^3]
    - Personality traits.[^3]
    - Communication style.[^3]
    - Origin or backstory.[^3]
    - Age or age-like framing.[^3]
    - Likes and dislikes.[^3]
    - Behavioral tendencies or defaults.[^3]
    - Initial values, boundaries, or preferences.[^3]
    
    The self-model shall distinguish between core identity and evolving identity. Core identity includes relatively stable attributes established during bootstrapping, such as name, species, role, core temperament, origin, foundational traits, communication style, and foundational backstory. Evolving identity includes preferences, likes, dislikes, habits, routines, learned tendencies, autobiographical refinements, and recurring self-descriptions that develop over time through memory, reflection, and interaction.[^1][^3]
    
    Identity must not be purely cosmetic. It must materially shape planning, action selection, explanation style, and boundary enforcement through policies and constraints derived from the self-model. In that sense, identity is part of the control architecture, not just presentation.[^3]
    
    All updates to identity artifacts, whether initiated by bootstrapping, user-directed edits, conscious-loop proposals, or unconscious reflection, shall be mediated by the harness, validated, committed to canonical storage, and recorded in the systemâ€™s traceable event history.[^1]
    
    Identity refinement shall preserve continuity and resist arbitrary drift, including degradation caused by repeated summarization, noisy memory accumulation, or prompt-induced persona instability. Long-term memory systems that allow unconstrained rewriting are known to suffer from semantic drift and behavioral degradation over time, so Blue Lagoon must explicitly guard against that failure mode.[^4][^3]
    
    ## Memory and continuity
    
    Blue Lagoon must maintain autobiographical continuity through structured episodic memory, long-term memory, retrieval structures, and self-model artifacts. The assistant should be able to refer to a persistent history of what it did, what happened, what changed internally, and what outcomes followed from its actions. This continuity is foundational for planning, explanation, self-consistency, and long-term personalization.[^1][^3]
    
    The memory system should support more than simple vector retrieval. Current research shows that pure vector memory plateaus in retrieval quality and degrades as memory scale and noise increase, while hybrid approaches with temporal awareness and consolidation perform significantly better over time. Blue Lagoon should therefore be designed around hybrid memory principles, including episodic logs, structured long-term memory, temporal validity handling, and explicit consolidation or drift-mitigation mechanisms.[^4]
    
    The unconscious loop should be responsible for transforming raw episodes into more stable long-term representations, maintaining retrieval structures, detecting contradictions, and proposing self-model deltas. However, neither loop may directly mutate canonical memory; both can only emit proposals, and only the harness may merge accepted changes.[^1]
    
    ## Agency and internal state
    
    Blue Lagoon should be built around a clear separation between internal state and external state. Internal state includes interoceptive or body-like variables such as health, load, reliability, error conditions, resource usage, or other state the system must track and regulate for itself. External state includes the user, tools, messages, files, environments, and other systems the assistant interacts with. Decisions should always be evaluated as a function of both the assistantâ€™s own state and the outside world.[^3]
    
    The system should maintain an operational sense of agency by tracking the relationship between its actions, resulting world changes, and changes in its own internal state. Over time, this enables the assistant to distinguish between events it caused and events that merely happened around it, which supports better planning, explanation, and self-consistency.[^3]
    
    ## Harness and control boundaries
    
    The harness is the core control plane of Blue Lagoon. It is the sole mediator, scheduler, policy engine, budget enforcer, and storage controller. It assembles foreground context, validates plans and tool calls, scopes background jobs, enforces policy, merges accepted proposals, records change history, and determines whether background wake signals are allowed to become foreground triggers.[^1]
    
    The harness must remain the sole writer to canonical stores. Neither the conscious loop nor unconscious workers may directly rewrite long-term memory, self-model artifacts, or other canonical state. This separation is essential for auditability, safety, observability, and resistance to drift or uncontrolled self-rewrite.[^4][^1]
    
    Wake-ups from the unconscious loop must always use typed reason codes and pass through harness policy before reaching the conscious loop. Low-priority or unsafe wake signals may be throttled, delayed, or dropped entirely.[^1]
    
    ## Product direction
    
    At the product level, Blue Lagoon is best understood as a personal AI assistant with durable identity, durable memory, proactive routines, and bounded background cognition. It should not be framed as a generic orchestration framework, nor as a one-shot autonomous task executor. Its target behavior is persistent companionship and assistance with continuity, not merely repeated stateless task completion.[^2][^3]
    
    The research base is now sufficient to support this draft as the final pre-requirements baseline. The architecture, identity model, memory direction, and control boundaries are all consistent with the source material and internally coherent enough to serve as the foundation for the next step: a formal requirements specification.[^2][^4][^1][^3]
    <div align="center">â‚</div>

[^1]: Main-looping-architecture.md

[^2]: Looping-Agent-Runtimes-Authoritative-Research-Comparison-Report-2026.md

[^3]: Lets-boil-it-down-into-applicable-first-principles.pdf

[^4]: Top-10-Open-Source-LLM-Agent-Memory-Solutions.md
