# Looping Agent Runtimes: Authoritative Research & Comparison Report (2026)

## Taxonomy and Scope

This report covers **true looping agent runtimes** â€” software that autonomously implements a Perceive â†’ Plan â†’ Act â†’ Observe â†’ Loop cycle, rather than frameworks that merely provide libraries *for building* such a loop. The distinction is architectural and consequential:

- A **true looping agent runtime** is a process that runs a cognitive loop on its own, takes actions against real systems, maintains persistent state, and can operate without a human triggering each individual step.
- An **orchestration framework** (LangGraph, CrewAI, LangChain, n8n, etc.) provides building blocks and execution scaffolding so *developers* can construct such a loop. They do not loop autonomously by themselves.

Applying this taxonomy, the true looping agent runtime ecosystem in 2026 breaks into two sub-categories:

**Category A â€” Always-On Persistent Daemons**: Run 24/7, connect to communication channels, respond to incoming messages *and* self-initiate scheduled/proactive tasks. The agent lives continuously on a host machine.

**Category B â€” Task-Triggered Deep-Loop Runtimes**: Accept a goal, spin up a full autonomous loop (which may run for tens or hundreds of iterations), complete the task, then halt. No persistent daemon; triggered per-task.

The ten runtimes covered in detail: **OpenClaw, NanoClaw, Nanobot, PicoClaw, ZeroClaw, NullClaw, NemoClaw** (Category A), and **Hermes Agent, Hugging Face smolagents, LightAgent** (Category B). A brief section covers additional notable runtimes.

***

## The OpenClaw Lineage

The entire Category A ecosystem traces back to a single weekend project by Austrian developer Peter Steinberger, released under the name Clawdbot (later Moltbot, then OpenClaw) in late 2025. OpenClaw's explosive viral growth â€” 250,000 GitHub stars in 60 days, faster than React's entire decade-long rise â€” triggered a Cambrian explosion of rewrites, optimizations, and security-hardened alternatives, all of which share OpenClaw's core interaction model but differ dramatically in language, resource footprint, and design philosophy.[^1][^2][^3]

An OSSInsight analysis of the four major forks (NanoClaw, Nanobot, ZeroClaw, PicoClaw) reached a key conclusion: taken together, these four projects accumulated 116,000 stars in 8 weeks, signaling that personal AI assistants had become a distinct software *category*, not just a trend.[^2]

***

## Category A: Always-On Persistent Daemons

### 1. OpenClaw

**Language**: TypeScript / Node.js | **License**: Open-source (transitioning to independent foundation) | **GitHub Stars**: 335,000+ (March 2026)[^3]

#### Origin and Philosophy

Created by Peter Steinberger in late 2025; Steinberger was subsequently acqui-hired by OpenAI. OpenClaw's philosophy is that AI should be **infrastructure, not an app** â€” a headless daemon running 24/7 on personal hardware (Mac Mini, VPS, Raspberry Pi), accessible via the messaging apps users already use, acting proactively as a "personal employee". Jensen Huang of NVIDIA called OpenClaw "the single most important software release, probably ever".[^4][^5][^1]

#### Architecture

Four-layer architecture:[^6][^7]

1. **Gateway Layer**: WebSocket/API server normalizing 15+ messaging platforms (WhatsApp, Telegram, Discord, Slack, iMessage, Teams, etc.) into a standard internal `Message` object
2. **Agent Core (Reasoning Engine)**: Model-agnostic LLM router (OpenAI, Anthropic, Ollama, local models)
3. **Tool Registry (Skills)**: TypeScript/JavaScript functions with lifecycle hooks (`before_agent_start â†’ agent_end`, `before_tool_call â†’ after_tool_call`)
4. **Memory System**: JSONL + Markdown files (human-readable, git-backable) + vector store (hybrid retrieval)

Key architectural decisions:
- **Lane Queue Concurrency**: Default serial execution per user lane, explicit parallelism for idempotent tasks â€” prevents race conditions[^6]
- **Semantic Snapshots**: Parses accessibility trees (ARIA) instead of screenshots for web browsing â€” claimed 90% token cost reduction[^6]
- **Heartbeat Tasks**: Self-initiated scheduled actions (cron-style) â€” enables proactive behavior independent of user messages

#### Community and Ecosystem

The community scale is unmatched in this category:
- 335,000+ GitHub stars; 65,300+ forks[^3]
- 1.5M weekly npm downloads[^8]
- 5,700+ community-developed skills; 1,100+ official skills on ClawHub[^8]
- 430,000+ lines of codebase â€” the largest by a significant margin[^9]

In China, adoption outpaced the United States, with major tech firms and local governments deploying OpenClaw-based agents at scale.[^1]

#### Security â€” The Critical Failure

OpenClaw's security record in 2026 is the most significant negative data point in this report, and must be understood by anyone evaluating the ecosystem:

- **Mass exposure**: 135,000â€“220,000+ instances exposed to the public internet due to default configuration binding to all network interfaces[^10][^11]
- **5 published CVEs**: CVE-2026-25253, CVE-2026-24763, CVE-2026-26322, CVE-2026-26329, CVE-2026-30741 â€” covering token leakage, command injection, SSRF, path traversal, and prompt injection-driven remote code execution[^12]
- **Malicious skill ecosystem**: "ClawHavoc" campaign â€” 824 confirmed malicious skills in the marketplace[^13]
- **Real-world incidents**: Documented cases of agents mass-deleting user email, inadvertently distributing $400,000, and installing malware â€” all while following legitimate-seeming instructions[^1]
- **Permissionless architecture**: No OS-level sandbox; agent has full access to the host filesystem unless explicitly restricted

The core problem is architectural: OpenClaw's security model is **application-level** (allowlists, policy rules) rather than **OS-level** (container isolation). Application-level controls are inherently bypassable through prompt injection â€” a fact that motivated every major fork.[^14][^15]

#### Pros

- Largest community and skill ecosystem by far
- Only truly always-on, multi-channel, proactive agent runtime at this community scale
- Full local execution, data sovereignty[^5]
- Browser automation via Chromium CDP (only framework in category with full browser control)[^16]
- Richest feature set (media handling, proactive scheduling, multi-agent swarms)

#### Cons

- Critical security vulnerabilities; 5 CVEs in 2026[^12]
- 430K-line codebase is difficult to audit[^4]
- High resource requirements (~1GB+ RAM, requires Mac Mini or equivalent)[^17]
- Slow cold start (~8 seconds)[^18]
- Malicious skills in marketplace require manual vetting[^13]

***

### 2. NanoClaw

**Language**: TypeScript / Node.js | **License**: MIT | **GitHub Stars**: 25,000+ (March 2026)[^19]

#### Origin and Philosophy

Created by Gavriel Cohen (co-founder, Qwibit), launched January 31, 2026. The founding premise: **don't trust the agent** â€” assume the AI is compromised and build the runtime accordingly. Every skill runs in a fully isolated OS-level container (Apple Containers on macOS, Docker on Linux), making the agent's blast radius bounded by the container boundary regardless of how it was manipulated.[^15][^14]

NanoClaw achieved 7,000+ stars in its first week and 11,000 in less than a month. By late March 2026 it crossed 25,000 stars. The fork ratio (forks/stars) is 1.76x higher than OpenClaw's and 2.5x higher than ZeroClaw's â€” indicating that developers are treating it as a base for custom specialized agents rather than using it directly.[^19][^2][^4]

#### Architecture

- **Container-first isolation**: Every skill execution creates a fresh container. Agents interact only with directories explicitly mounted by the user[^15]
- **Ephemeral containers**: Created per-invocation, destroyed after use â€” prevents persistence attacks where a compromised agent leaves backdoors[^14]
- **Context separation**: Separate containers for personal, work, and family agent contexts â€” a compromised work agent cannot leak data to a personal agent[^14]
- Runs on Anthropic's Agent SDK for reasoning[^4]
- ~3,900 lines of core TypeScript â€” readable in 8 minutes[^4]
- **Memory ~400MB** â€” larger than Rust/Go alternatives but dramatically simpler than OpenClaw[^20]

#### Key Features

- OS-level sandbox per skill (primary differentiator)[^15]
- 5+ messaging channels: WhatsApp, Telegram, Discord, Slack, Signal[^4]
- Agent vault â€” first in the Claw ecosystem to launch a shared vault for multi-agent state[^19]
- MCP proxy with per-tool permission scoping (SpiceDB-backed authorization)[^21]
- ~15MB binary[^20]

#### Pros

- Only TypeScript Claw with OS-level isolation â€” direct OpenClaw migration path[^20]
- Simple enough to audit fully (8-minute read time)[^4]
- Directly addresses OpenClaw's most critical CVEs architecturally
- High fork ratio means a growing ecosystem of specialized derivatives[^2]

#### Cons

- ~400MB memory (heavier than Rust/Go alternatives)
- Smaller skill ecosystem than OpenClaw
- Container overhead adds latency per skill execution[^14]
- Requires Docker or Apple Containers â€” not suitable for minimal environments[^15]

***

### 3. Nanobot

**Language**: Python | **License**: Open-source | **GitHub Stars**: ~36,283 (March 2026)[^2]

#### Origin and Philosophy

Developed by the HKUDS Data Intelligence Lab at the University of Hong Kong, released February 2, 2026. Philosophy: **radically transparent AI assistant infrastructure** â€” the entire codebase should be readable by any competent developer, making it auditable, forkable, and trustworthy by design. At ~3,806 lines of core Python (verified via `core_agent_lines.sh` script in the repo), Nanobot is 99% smaller than OpenClaw's 430K+ lines while delivering equivalent core functionality.[^22][^9]

With 204 contributors and a fork ratio close to OpenClaw's (0.171 vs 0.196), Nanobot has the most engaged contributor community relative to its size in the Claw family.[^2]

#### Architecture

- Pure Python 3.10+; `pip install nanobot-ai` or `uv tool install nanobot-ai`[^9]
- **Provider Registry**: Adding a new LLM provider requires exactly 2 steps[^9]
- **Heartbeat system**: Redesigned in v0.1.4 using a virtual tool-call decision mechanism â€” silent when idle, avoiding false positives in loop detection[^23]
- **Prompt cache optimization**: Dynamic context (time, session) moved from system prompt to user message for persistent cache hits[^23]
- Memory: channel-level and cross-session persistence

#### Key Features

- 8+ messaging platforms: Telegram, Discord, WhatsApp, Feishu, Mochat, DingTalk, Slack, Email, QQ[^9]
- 11+ LLM providers: OpenRouter, Anthropic, OpenAI, DeepSeek, Gemini, Zhipu, DashScope, Moonshot, Groq, AiHubMix, vLLM[^9]
- Parallel + sequential tool execution in single-turn (community-contributed Universal Tool Orchestrator patch)[^24]
- 2-minute deployment[^25]
- ~100MB+ RAM requirement[^17]

#### Pros

- Most readable codebase in the category (3,806 lines)
- Fast setup (2 minutes), no complex dependencies
- Broadest Asian platform support (Feishu, DingTalk, QQ, Mochat) â€” important for deployments in Chinese enterprise environments
- Highly forkable (204 contributors, clean architecture)[^2]
- Best for researchers and developers who want to study agent internals

#### Cons

- Python runtime overhead (~100MB+ RAM vs <10MB for Go/Rust alternatives)
- No OS-level sandboxing
- Fewer channels than ZeroClaw/NullClaw
- Community-contributed features (like parallel tool orchestration) not yet merged to main[^24]

***

### 4. PicoClaw

**Language**: Go | **License**: Open-source (Sipeed) | **GitHub Stars**: ~25,000 (March 2026)[^26]

#### Origin and Philosophy

Created by Sipeed (maker of embedded AI hardware like the Maix series boards) and launched February 9, 2026 â€” built in a single day. The philosophy is extreme portability: a **single binary that runs anywhere** â€” RISC-V, ARM, MIPS, x86 â€” targeting the $10 hardware market that OpenClaw's Node.js runtime categorically excludes. An unusual design choice: 95% of PicoClaw's core code was generated by an AI agent itself through a "self-bootstrapping" process with human-in-the-loop review.[^27][^28]

PicoClaw hit 20,000 stars in its first 17 days.[^29]

#### Architecture

- Go binary, single-file deployment; <10MB RAM[^28]
- **Smart model routing**: Rule-based routing sends simple queries to lightweight models, expensive queries to capable models â€” reduces API cost significantly[^27]
- **MCP-native**: Model Context Protocol support added in v0.2.1 â€” connects any MCP server to extend agent capabilities[^27]
- Vision pipeline: Send images and files directly; automatic base64 encoding for multimodal LLMs[^27]
- JSONL memory store[^27]
- Web UI launcher + Docker Compose support (v0.2.0+)[^29]

#### Key Features

- 400x faster startup than OpenClaw on 0.6GHz single-core hardware[^28]
- <1 second cold start on $10 Raspberry Pi boards[^16]
- 400x faster startup than OpenClaw; 100x lower memory[^30]
- Multi-architecture: same binary for RISC-V, ARM, x86 â€” no cross-compile needed per platform[^28]
- 10+ channels including Matrix, IRC, WeCom, Discord Proxy (v0.2.1)[^27]

#### Pros

- Best choice for embedded/IoT/edge deployments
- Single-binary simplicity (no dependency management)
- MCP-native from v0.2.1 (ahead of most peers)
- Smart model routing reduces cost for mixed workloads

#### Cons

- No browser automation (vs OpenClaw's Chromium CDP)[^16]
- Multi-agent/swarm features only basic (sub-agent spawning)[^16]
- Agent Refactor still in progress (loop + events redesign)[^31]
- 95% AI-generated code introduces potential quality concerns; audit coverage uncertain

***

### 5. ZeroClaw

**Language**: Rust | **License**: MIT + Apache 2.0 (dual) | **GitHub Stars**: ~15,200 (February 2026)[^18]

#### Origin and Philosophy

Created by Kai Tanaka (former Mozilla engineer) and twelve contributors, launched February 2026. ZeroClaw is a **ground-up Rust rewrite** of OpenClaw's architecture â€” not a port, but a complete reconstruction of the gateway server, skill execution engine, message routing, and LLM client in memory-safe Rust. It preserves behavioral compatibility (reads OpenClaw SKILL.md files natively, connects to the same messaging platforms) while delivering transformational performance improvements.[^18]

The dual MIT/Apache 2.0 license is a practical advantage for enterprise deployment.[^32]

#### Architecture

- **Trait-driven design**: Every subsystem (providers, channels, tools, memory, observability) implements a Rust trait â€” fully swappable without changing agent logic[^32]
- **Three operating modes**:[^33]
  - **Agent mode (CLI)**: Single agent from command line; ideal for scripting and CI/CD
  - **Gateway mode (HTTP)**: Agents exposed as HTTP endpoints; other services trigger agent actions via API
  - **Daemon mode**: Full 24/7 runtime with gateway + channel integrations + heartbeat + cron scheduler
- **Built-in supervisor**: Daemon auto-restarts on crashes[^34]
- **`zeroclaw doctor`**: One-command diagnostic for broken channels, missing dependencies, misconfigured permissions[^34]
- **`zeroclaw migrate openclaw`**: Direct OpenClaw config and memory store migration[^35]

#### Performance (Benchmarked vs OpenClaw)[^18]

| Metric | OpenClaw | ZeroClaw | Improvement |
|--------|----------|----------|-------------|
| Cold start | 8.2s | 0.6s | 14x |
| Idle memory | ~487 MB | ~36 MB | 14x |
| Binary size | 150MB+ | 8.8MB | 17x |
| Skill execution | baseline | 14x faster | 14x |

ZeroClaw processes 40% lower token costs in repeated queries due to persistent memory/context rebuilds. It achieves p50 latency of 40ms vs OpenClaw's 150ms at low concurrency.[^36]

#### Key Features

- 22+ AI provider integrations (OpenAI, Anthropic, OpenRouter, custom endpoints)[^32]
- 15+ messaging channels[^37]
- Pairing-based gateways, strict sandboxing, explicit allowlists, workspace scoping, encrypted secrets at rest, built-in rate limiting[^37]
- Prometheus + OpenTelemetry observability built-in[^32]
- SQLite hybrid search (keyword + vector) for memory[^32]
- 85% OpenClaw skill compatibility with WebAssembly sandboxing[^18]

#### Pros

- 14x performance improvement over OpenClaw â€” transformational for constrained hardware
- Memory-safe by construction (Rust ownership model eliminates entire vulnerability classes)
- Built-in observability (Prometheus/OTel) â€” production-grade from day one
- Seamless OpenClaw migration path[^34]
- Dual license suitable for commercial use

#### Cons

- 85% skill compatibility means 15% of OpenClaw skills require porting[^18]
- Smaller community than Python/TypeScript alternatives (Rust barrier to contribution)
- No browser automation[^38]
- Multi-agent features missing vs OpenClaw[^18]

***

### 6. NullClaw

**Language**: Zig | **License**: MIT | **GitHub Stars**: Not prominently cited (early-stage project)

#### Origin and Philosophy

NullClaw implements a full-stack AI agent in raw Zig â€” a systems programming language with no hidden control flow, no garbage collector, no runtime, and zero dependencies beyond libc. The engineering thesis: the overhead introduced by managed-language runtimes (Python's GIL, Node.js's event loop, Go's GC) is not a necessary cost of running an AI agent. NullClaw proves it can be eliminated entirely while retaining a complete, feature-rich agent stack.[^39][^17]

The result is the most extreme embodiment of minimalism in this category: **678 KB binary, ~1 MB RAM, <2ms boot time**. This is not a stripped-down demo â€” the codebase is 245 source files, ~204,000 lines of Zig, with 5,640+ tests.[^40][^39][^17]

#### Architecture

- **Vtable-driven modularity**: Every subsystem (providers, channels, memory, tools, observability, hardware peripherals) is a vtable interface. Swapping OpenAI for local DeepSeek requires a config change only â€” zero code modifications[^39][^40]
- Extension points:[^40]
  - `src/providers/root.zig` â€” AI model providers
  - `src/channels/root.zig` â€” messaging channels
  - `src/tools/root.zig` â€” tool execution surface
  - `src/memory/root.zig` â€” memory backends
  - `src/observability.zig` â€” observability hooks
  - `src/runtime.zig` â€” execution environments
  - `src/peripherals.zig` â€” hardware boards (Arduino, STM32, RPi)
- **Memory management**: Manual (Zig-native). Hybrid vector + keyword search via a local SQLite driver[^41]
- **2,738+ tests** ensuring memory safety guarantees in a manually managed language[^39]

#### Key Features

- 22+ AI providers (OpenAI, Anthropic, Ollama, DeepSeek, Groq, and more)[^41]
- 18+ communication channels including Telegram, Discord, Slack, WhatsApp, iMessage, IRC, voice[^41]
- Streaming, voice, multimodal support[^41]
- Hardware-native: Arduino, Raspberry Pi, STM32 peripherals built-in[^39]
- Multi-layer sandboxing[^41]
- Runs on a $5 development board[^39]

#### Pros

- The absolute minimum viable hardware for a full-featured AI agent ($5 board)[^39]
- Zero external runtime dependencies â€” no Python, JVM, Go, or Node.js required
- Near-instant cold start (<2ms) enables serverless/event-driven use cases
- Rigorous test suite (5,640+ tests) compensates for manual memory management risks
- Broadest hardware peripheral support in the ecosystem

#### Cons

- Zig is an immature language (pre-1.0 at time of writing) with limited tooling and smaller talent pool
- Manual memory management creates a high contribution barrier â€” fewer contributors expected
- Community size data not well-reported; adoption metrics uncertain
- Zig's evolving language specification means breaking changes are possible

***

### 7. NemoClaw

**Language**: Python + NVIDIA NeMo/NIM stack | **License**: Apache 2.0 (core) + paid enterprise tier | **GitHub Stars**: ~4,600 (released March 16, 2026)[^42]

#### Origin and Philosophy

Announced by Jensen Huang at GTC 2026 on March 16, 2026. NemoClaw is NVIDIA's strategic play to do with AI agent infrastructure what CUDA did with GPU programming â€” establish a dominant platform layer. It is an **enterprise governance and fleet orchestration layer** built on top of OpenClaw's agent model, adding security, compliance, RBAC, and fleet management that enterprises require.[^43][^44][^42]

The strategic positioning: OpenClaw for developers â†’ NemoClaw for enterprise deployment. Same agent primitives, enterprise wrapper.[^43]

#### Architecture

- Built on the **NeMo framework** (model training + reasoning pipelines) + **Nemotron models** (default: Nemotron 3 Super, 120B total / 12B active parameters) + **NIM inference microservices**[^45]
- **OpenShell runtime**: Enforces security at the infrastructure level, not the application level â€” every agent action is evaluated against enterprise policy before execution[^43]
- **Agent registration**: Every agent must declare capabilities, tool access, and authorization scope before execution[^43]
- **Immutable audit trail**: All actions logged to an append-only audit log[^43]
- **4-layer isolation**: Network, filesystem, process, inference[^42]
- **Local + cloud model routing** ("local and cloud model foundation"): Sensitive tasks route to local Nemotron models; complex tasks to cloud models via privacy router[^44]

#### Fleet Management Features[^43]

- Deploy and manage hundreds of concurrent agents across NVIDIA-powered infrastructure
- Load balancing, health monitoring, automatic restart on failure, version management
- Supervisor-worker agent hierarchies, shared memory between agents, structured handoffs
- Synchronous and asynchronous multi-agent workflows

#### Hardware and Licensing

- **Apache 2.0 open-source core** â€” free to download and use[^46]
- **Hardware-agnostic**: Runs on NVIDIA, AMD, Intel GPUs and major cloud instances â€” deliberate strategy to avoid lock-in[^47][^46]
- **Optimized for**: DGX Station, DGX Spark, GeForce RTX, RTX PRO workstations[^47]
- **Paid enterprise tier**: Managed infrastructure, compliance tooling, SLAs[^46]
- **Minimum requirements**: 4+ vCPUs, 8 GB RAM[^42]

#### Pros

- Only enterprise-grade agent platform with built-in RBAC, audit trails, and fleet orchestration
- Hardware-agnostic despite being NVIDIA-branded
- Nemotron models are production-proven (deployed by CrowdStrike, Cursor, Deloitte, Oracle, Palantir, Perplexity, ServiceNow)[^45]
- Apache 2.0 license enables commercial embedding
- Integrates with existing NVIDIA developer tooling

#### Cons

- Very new (alpha/early-access as of March 26, 2026)[^42]
- Effectively requires GPU infrastructure for meaningful deployment[^48]
- Small community (4,600 stars) â€” ecosystem still nascent
- Enterprise tier costs significant infrastructure investment ($2Kâ€“$50K GPU hardware)[^48]
- Heavy coupling to NVIDIA's NeMo/NIM/Nemotron stack

***

## Category B: Task-Triggered Deep-Loop Runtimes

These runtimes implement genuine autonomous looping â€” the agent receives a goal and loops independently through reason/act/observe cycles until done â€” but are triggered per-task rather than running as persistent daemons.

### 8. Hermes Agent (Nous Research)

**Language**: Python | **License**: MIT | **GitHub Stars**: ~11,000+ (first month, 874 stars/day at peak)[^49]

#### Origin and Philosophy

Released by Nous Research in February 2026. Hermes Agent occupies an explicit middle position in the market: "between a Claude Code style CLI tool and an OpenClaw style messaging platform agent". It is designed for the **self-improving agent** use case â€” not just executing tasks, but learning from them, creating new skills autonomously, and improving those skills over time through a closed learning loop.[^50][^51]

Beyond its standalone utility, Hermes Agent directly powers Nous Research's **Atropos** reinforcement learning pipeline â€” the agent's execution primitives are used to generate synthetic training data at scale, making it simultaneously a research tool and a product.[^52]

#### The Closed Learning Loop[^49][^50]

The defining architectural feature: the agent curates its own memory, creates new skills from complex tasks using the open `agentskills.io` format, refines those skills during use, and maintains FTS5 full-text cross-session recall with LLM summarization. The Honcho dialectic user modeling system builds an understanding of the user's preferences over time.

This is fundamentally different from other runtimes: Hermes is not just executing a fixed skill set â€” it is *growing* one.

#### Architecture

- **Agent loop internals**: Iteration budget tracking shared across parent and subagents; budget pressure hints near iteration limit; fallback model switching when primary provider fails[^53]
- **6 execution backends**: Local, Docker, SSH, Daytona, Singularity, Modal â€” serverless backends (Daytona, Modal) hibernate when idle[^50]
- **Messaging gateway**: Runs in CLI and connects to Telegram, WhatsApp, Slack, Discord[^52]
- **40+ bundled skills**: MLOps, GitHub workflows, research â€” plus autonomous skill creation[^49]
- Version 0.3.0 (March 17, 2026): Unified real-time token delivery, first-class plugin architecture, rebuilt provider system[^49]

#### Security Model (5 layers)[^54]

1. User authorization: allowlists, DM pairing with cryptographic codes (8-char from 32-char unambiguous alphabet, 1-hour TTL, rate-limited)
2. Dangerous command approval: pattern-matching detects destructive operations; 45-second timeout in CLI; messaging gateway approval flow
3. Container isolation: Docker/Singularity/Modal with hardened settings (read-only filesystem, dropped capabilities, PID limits)
4. MCP credential filtering: environment variable isolation for MCP subprocesses
5. Context file scanning: prompt injection detection in project files

**Iteration budget enforcement**: Default 90 turns per conversation; warnings at 70% and 90% usage; forced termination for runaway processes.[^55]

#### Pros

- Unique self-improving capability â€” agent creates and refines its own skills
- Multi-modal execution backends including serverless (cost-effective for intermittent use)
- Exceptionally detailed security model with formal approval flows
- Dual role as research tool (Atropos RL pipeline) and production agent
- Zero-telemetry architecture â€” privacy-first

#### Cons

- Task-triggered, not persistent (no 24/7 daemon mode)
- Less mature ecosystem than OpenClaw (40+ skills vs 5,700+)
- Skill creation quality depends on underlying model capability
- 90-turn budget limits very long autonomous tasks

***

### 9. Hugging Face smolagents

**Language**: Python | **License**: Apache 2.0 | **GitHub Stars**: ~15,000 (March 2025 milestone)[^56]; growing through 2026

#### Origin and Philosophy

Developed by Hugging Face, released in late 2024. Philosophy: **radical minimalism** â€” the entire agent logic fits in approximately 1,000 lines of code. The signature innovation is the **CodeAgent** paradigm: instead of generating JSON to describe tool calls, the agent generates and executes Python code directly. A single generated Python block can write loops, define variables, chain multiple tools, and call APIs â€” achieving more expressive actions in fewer LLM steps.[^57][^58][^59][^60]

#### The CodeAgent Paradigm

The performance case for code-as-actions:[^61]
- **30% fewer steps** than JSON-based tool calling for equivalent tasks
- Single Python block can express multi-tool compositions that would require multiple JSON tool-call rounds
- Generated code is auditable â€” you can read exactly what the agent is about to execute
- Natural integration with the scientific computing ecosystem (numpy, pandas, etc.)

A parallel `ToolCallingAgent` is available for scenarios where JSON-based tool calling is preferred (less capable models, stricter sandboxing requirements).[^59]

#### Architecture

- ~1,000 lines of core Python â€” deliberately maintained at this scale[^59]
- **Model-agnostic**: HuggingFace Inference API, OpenAI-compatible APIs, Anthropic, LiteLLM, local Transformers, local Ollama[^62]
- **Sandboxed execution**: E2B or Docker for isolating generated code[^56]
- **Hub integration**: Share and load agents and tools as Gradio Spaces[^59]
- Multi-agent: agents call other agents as tools; full hierarchical composition[^62]
- Structured outputs internally (Qwen3 / structured generation support)[^63]
- Intermediate planning steps for complex tasks[^62]

#### Key Features

- Code-as-actions paradigm (primary differentiator)
- Native HuggingFace ecosystem integration (hundreds of open-source models available immediately)
- Visual Gradio interface for agent demos and sharing
- Lifecycle hooks for custom execution flow monitoring (PR in review)[^64]
- MCP integration for tool connectivity

#### Pros

- Best integration with the open-source model ecosystem (HuggingFace Hub)
- Code-as-actions delivers 30% fewer steps than JSON tool-calling[^61]
- Minimal, auditable codebase (~1,000 lines)
- Strong institutional backing (HuggingFace)
- Sandboxed execution via E2B/Docker

#### Cons

- Task-triggered, not always-on
- Loop can stall after multiple iterations with certain local models[^65]
- No persistent daemon / messaging gateway
- Less suitable for non-coding tasks where code generation is unnecessary overhead
- Lifecycle hooks incomplete (not yet fully supported mid-execution)[^64]

***

### 10. LightAgent

**Language**: Python | **License**: Open-source | **GitHub Stars**: Modest (~hundreds to low thousands)[^66]

#### Origin and Philosophy

Developed by wxai-space and documented in academic paper arXiv:2509.09292. LightAgent's positioning is **active learning and autonomous growth**: each agent possesses autonomous learning capabilities, updates its own knowledge base from interactions, and generates new tools dynamically from API documentation input. The Tree of Thought (ToT) built directly into the agent loop is the primary technical differentiator â€” enabling the agent to explore multiple reasoning branches before committing to an action.[^67][^68]

#### Architecture

- **Tree of Thought (ToT)** as core loop structure: The agent builds a reasoning tree, evaluates branches through self-reflection, and selects the highest-scoring path[^67]
- A separate model (default: DeepSeek-R1) handles planning and thinking; the primary model handles execution â€” reducing overlong reasoning in the main loop[^67]
- **LightSwarm**: Multi-agent subsystem that automatically registers agents for a task, synchronizes memory, parses intent, and coordinates sub-agents[^68]
- **mem0 memory integration**: Long-term, personalized memory that updates from interactions[^67]
- **Automated tool generation**: Ingest API documentation â†’ generate hundreds of domain-specific tools in under an hour[^68][^67]

#### Key Features

- Tree of Thought reasoning built into the execution loop
- Automated tool generation from API docs
- Self-learning per agent (autonomous knowledge base updates)
- MCP + SSE protocol integration[^69]
- Supports OpenAI, DeepSeek, Qwen, ChatGLM, Baichuan[^70]
- Chat platform deployment: designed for integration into conversational interfaces

#### Pros

- Most sophisticated built-in reasoning (Tree of Thought)
- Automated tool generation dramatically reduces time to new capabilities
- Academic foundation provides theoretical grounding[^66]
- Self-learning reduces manual maintenance over time

#### Cons

- Small community and limited production adoption evidence
- No persistent daemon mode
- Dual-model setup (ToT + execution) increases inference cost
- Less tested at scale than other entries in this report

***

## Comparative Analysis

### Resource and Performance Matrix

| Runtime | Language | Binary/RAM | Cold Start | Stars | Category |
|---------|----------|-----------|------------|-------|----------|
| OpenClaw | TypeScript | ~150MB+ / 1GB+ | 8.2s[^18] | 335K+[^3] | A |
| NanoClaw | TypeScript | ~15MB / ~400MB | Seconds | 25K+[^19] | A |
| Nanobot | Python | N/A / ~100MB | ~30s | ~36K[^2] | A |
| PicoClaw | Go | ~10MB / <10MB | <1s[^28] | ~25K[^26] | A |
| ZeroClaw | Rust | 8.8MB / ~36MB | 0.6s[^18] | ~15K[^18] | A |
| NullClaw | Zig | 678KB / ~1MB | <2ms[^39] | N/A | A |
| NemoClaw | Python+NIM | GPU-scale | Seconds | ~4.6K[^42] | A |
| Hermes Agent | Python | N/A / N/A | Seconds | ~11K+[^49] | B |
| smolagents | Python | N/A / N/A | Seconds | ~15K[^56] | B |
| LightAgent | Python | N/A / N/A | Seconds | Low | B |

### Feature Matrix

| Runtime | Always-On | Multi-Channel | Skill Ecosystem | OS Sandbox | MCP | Browser | Self-Learning |
|---------|-----------|--------------|-----------------|-----------|-----|---------|---------------|
| OpenClaw | âœ… | âœ… 15+[^5] | âœ… 5,700+[^8] | âŒ | âœ… | âœ… CDP[^16] | âŒ |
| NanoClaw | âœ… | âœ… 5+[^20] | Limited | âœ… OS-level[^15] | âœ…[^21] | âŒ | âŒ |
| Nanobot | âœ… | âœ… 8+[^9] | Growing | âŒ | Partial | âŒ | âŒ |
| PicoClaw | âœ… | âœ… 10+[^27] | Limited | âŒ | âœ… native[^27] | âŒ | âŒ |
| ZeroClaw | âœ… | âœ… 15+[^37] | 85% OpenClaw[^18] | âœ… Wasm[^18] | âœ… | âŒ | âŒ |
| NullClaw | âœ… | âœ… 18+[^41] | Limited | âœ… multi-layer[^41] | Partial | âŒ | âŒ |
| NemoClaw | âœ… Fleet | Enterprise[^43] | Limited | âœ… 4-layer[^42] | âœ… | âŒ | âŒ |
| Hermes Agent | âŒ Triggered | âœ… 4+[^52] | 40+ skills[^49] | âœ… Docker[^54] | âœ…[^54] | âŒ | âœ…[^50] |
| smolagents | âŒ Triggered | âŒ | HF Hub[^59] | âœ… E2B/Docker[^56] | âœ… | âŒ | âŒ |
| LightAgent | âŒ Triggered | Partial | Auto-generated[^67] | âŒ | âœ…[^69] | âŒ | âœ…[^67] |

### Security Architecture Comparison

Security is the defining fault line in this ecosystem, particularly following OpenClaw's 2026 exposure crisis.[^10]

| Runtime | Isolation Model | Notable Security Feature | CVEs (2026) |
|---------|----------------|--------------------------|-------------|
| OpenClaw | App-level (bypassable) | 7-tier policy; role-based scopes | 5 CVEs[^12] |
| NanoClaw | **OS-level container** per skill | Ephemeral containers; no persistence | None known[^15] |
| Nanobot | None | Readable codebase = auditability | None known |
| PicoClaw | None built-in | Credential isolation (in-progress)[^31] | None known |
| ZeroClaw | Wasm sandboxing | Pairing-based gateways; encrypted secrets at rest | None known[^37] |
| NullClaw | Multi-layer sandbox | Vtable isolation; 5,640+ tests[^40] | None known |
| NemoClaw | 4-layer (network/FS/process/inference) | OpenShell policy enforcement; immutable audit log | None (too new)[^43] |
| Hermes | 5-layer (see Â§8) | Cryptographic pairing; iteration budget enforcement | None known[^54] |
| smolagents | E2B / Docker code execution | Code is auditable before execution[^57] | None known |
| LightAgent | None | â€” | None known |

The key insight from security research: **application-level controls are inherently insufficient against prompt injection**. An adversarially manipulated prompt can instruct the agent to bypass allowlists, leak credentials, or exfiltrate data. Only runtimes with OS-level enforcement (NanoClaw, NemoClaw, Hermes via Docker backend) provide genuine containment guarantees.[^54][^14][^15]

### Scored Comparison

Scores are composite assessments (0â€“10) based on research evidence, weighted against the original criteria.

| Runtime | Feature Set | Performance | Extensibility | Community | Tech Stack | Code Quality | **Total /60** |
|---------|------------|-------------|---------------|-----------|-----------|--------------|-------------|
| OpenClaw | **10** | 5 | **10** | **10** | 6 | 4 | **45** |
| NanoClaw | 6 | 6 | 8 | 7 | 7 | **9** | **43** |
| Nanobot | 6 | 5 | **9** | 7 | 7 | **9** | **43** |
| PicoClaw | 6 | **9** | 7 | 7 | **9** | 7 | **45** |
| ZeroClaw | 7 | **9** | 8 | 6 | **10** | **9** | **49** |
| NullClaw | 7 | **10** | **9** | 4 | **10** | **9** | **49** |
| NemoClaw | 8 | 7 | 6 | 3 | 8 | 7 | **39** |
| Hermes Agent | 7 | 7 | 8 | 6 | 8 | **9** | **45** |
| smolagents | 7 | 7 | 8 | 7 | 8 | **10** | **47** |
| LightAgent | 6 | 6 | 7 | 3 | 7 | 7 | **36** |

*Notes: Code Quality penalizes OpenClaw for 5 CVEs and "vibe-coded" origins. Performance scores reflect resource efficiency and cold-start metrics, not LLM inference speed (which is model-dependent). Community scores reflect GitHub stars, contributor counts, and ecosystem depth. NemoClaw and LightAgent score lower on community as newly released projects.*

***

## Design Philosophy Taxonomy

The ten runtimes cluster around four philosophical axes:

### "Personal Employee" (OpenClaw, Hermes Agent)
The agent is your always-available assistant with broad, trust-based system access. Philosophy: maximize capability and convenience; security is a configuration concern. OpenClaw exemplifies maximum feature scope and permissiveness. Hermes Agent adds approval gates and learning, pulling toward the security-first direction.

### "Security-First" (NanoClaw, NemoClaw)
Trust nothing, contain everything. The agent is assumed adversarial; OS-level isolation is the only reliable defense. NanoClaw does this minimally (container per skill). NemoClaw does this at enterprise fleet scale (OpenShell policy engine, immutable audit).

### "Efficiency-First" (PicoClaw, ZeroClaw, NullClaw)
The managed-language runtime overhead is waste. Rebuild in Go/Rust/Zig, achieve 10-100x better resource efficiency, and unlock hardware categories that managed languages exclude entirely. The philosophical bet: simplicity and portability beat features and familiarity.

### "Cognitive-First" (LightAgent, smolagents)
The limiting factor is not runtime overhead or security â€” it is the quality of the agent's reasoning. Invest in better in-loop cognition: Tree of Thought (LightAgent), code-as-actions (smolagents), self-improving skill generation (both). The loop structure itself is the product.

***

## Use Case Decision Guide

### "I want a powerful, always-on personal agent with maximum features and community support"
**â†’ OpenClaw** â€” but only with explicit security hardening: change default bind address to 127.0.0.1, disable all unused skills, monitor CVE advisories, enable logging, rotate credentials regularly. The feature ecosystem and community support are unmatched.[^5][^8]

### "I want OpenClaw's capability but cannot accept its security exposure"
**â†’ NanoClaw** â€” same TypeScript/Node.js stack, same messaging channels, OS-level container isolation per skill. The codebase is simple enough to audit fully. The fork ratio suggests a growing ecosystem of specialized derivatives.[^19][^15]

### "I want to study or fork agent internals â€” readability is a hard requirement"
**â†’ Nanobot** â€” 3,806 lines of clean Python is the most auditable codebase in this category by a wide margin. 2-minute deployment.[^22][^25]

### "I need to deploy on edge hardware ($10â€“$50 boards, RISC-V, ARM, IoT)"
**â†’ PicoClaw** for Go simplicity and MCP support; **ZeroClaw** for maximum performance and OpenClaw migration; **NullClaw** for absolute minimal footprint ($5 boards, <2ms boot). Choose based on your language preference and hardware constraints.[^35][^28][^39]

### "I need production reliability with observability baked in"
**â†’ ZeroClaw** â€” Prometheus + OpenTelemetry built-in, built-in supervisor, `zeroclaw doctor` diagnostics, encrypted secrets at rest, and a 14x performance improvement over OpenClaw.[^37][^18]

### "I need enterprise deployment with compliance, RBAC, and fleet orchestration"
**â†’ NemoClaw** â€” the only option in this category with immutable audit trails, RBAC, fleet management, and enterprise SLAs. Requires GPU infrastructure investment.[^43]

### "I need a task-driven agent that learns and improves from its own execution history"
**â†’ Hermes Agent** â€” closed learning loop, autonomous skill creation, Honcho user modeling, multi-backend execution including serverless. Most sophisticated self-improvement architecture available.[^50]

### "I need an agent deeply integrated with open-source models and the HuggingFace ecosystem"
**â†’ smolagents** â€” code-as-actions (30% fewer LLM steps), Hub integration, ~1,000-line auditable core, sandboxed code execution.[^61][^59]

### "I need autonomous multi-step reasoning on complex goals with limited tool definition overhead"
**â†’ LightAgent** â€” Tree of Thought reasoning built into the loop, automated tool generation from API docs, self-learning per-agent knowledge base.[^68][^67]

***

## Additional Notable Runtimes

**MicroClaw** (Rust): A Rust multi-channel agent runtime originating as a Telegram bot, evolved into a production-grade architecture with a shared `agent_engine.rs`, provider-agnostic LLM layer, multi-step tool execution, session resume + context compaction, and background scheduling. Specializes in Asian and privacy-focused platforms: Feishu/Lark, Matrix, Nostr, Signal, DingTalk, QQ â€” channels the major runtimes lack.[^71][^33]

**IronClaw**: A security-hardened OpenClaw fork mentioned in coverage of the security crisis landscape; limited independent research available.[^1]

**obra/superpowers**: 110,000+ GitHub stars (March 2026); a Shell-based agent harness package with 19,621 stars/week growth rate at peak; positioned as a "ready-to-use agent harness." Categorization as a looping runtime requires further investigation.[^72]

***

## Cross-Cutting Observations

### The Security Debt of Fast Growth
OpenClaw's story is a case study in the security cost of viral adoption. The architectural decision to bind to all network interfaces by default â€” a reasonable choice for a weekend demo project â€” became a systemic vulnerability once the project had 220,000 deployed instances. The lesson for the ecosystem: **security defaults must be designed for the worst-case deployment, not the expected case**. NanoClaw's container-first approach, ZeroClaw's localhost-only gateway default, and NemoClaw's OpenShell enforcement all represent corrections of this original sin.[^10]

### The Codebase Size vs. Feature Trade-off
The Claw ecosystem presents a clear inverse relationship between codebase size and auditability: OpenClaw (430K lines, highest CVE count) vs Nanobot (3,806 lines, zero CVEs) vs NullClaw (204K lines of Zig, 5,640 tests, zero CVEs). Codebase size is not the sole determinant â€” NullClaw has a large, rigorously tested codebase â€” but complexity without tests is the specific risk factor.

### Protocol Convergence
MCP (Anthropic's Model Context Protocol) is becoming the interoperability standard for tools within this category. PicoClaw adopted it natively in v0.2.1; ZeroClaw, NullClaw, Hermes, and smolagents all support it. A2A (Google's Agent-to-Agent protocol) has not yet penetrated this category as of March 2026 â€” it is primarily active in the orchestration framework layer.[^27]

### The "Personal AI Category" Formation
The four major forks accumulated 116,000 stars in 8 weeks. The analyst interpretation: this is not fragmentation but ecosystem formation. OpenClaw proved the concept; the forks are proving that personal autonomous agents are a durable software category, not a viral moment. Each fork represents a distinct user persona: security-conscious developers (NanoClaw), researchers (Nanobot), embedded engineers (PicoClaw/NullClaw), performance engineers (ZeroClaw), enterprise architects (NemoClaw).[^2]

---

## References

1. [Don't Trust AI Agents, Says OpenClaw's Security-First Alternative ...](https://www.forbes.com/sites/gilpress/2026/03/13/dont-trust-ai-agents-says-nanoclaw-now-fully-integrated-with-docker/) - NanoClaw, the security-first AI agent platform that has surpassed 20,000 GitHub stars and 100,000 do...

2. [Four Teams Rewrote OpenClaw â€” Here's What the Code Says](https://ossinsight.io/blog/the-openclaw-forks-wave-2026) - NanoClaw has 8,778 forks against 25,489 stars. Compare this to the ... Data collected via GitHub API...

3. [How OpenClaw Became GitHub's Most-Starred Project in 60 Days](https://www.lowtouch.ai/openclaw-github-stars-agentic-ai-history/) - By March 2026, OpenClaw had crossed 335,000 GitHub stars, surpassing React's all-time cumulative rec...

4. [GitHub All-Stars #14: NanoClaw - VirtusLab](https://virtuslab.com/blog/ai/nano-claw-your-personal-ai-butler) - We're looking at NanoClaw: a lightweight, container-isolated personal AI assistant that connects to ...

5. [OpenClaw: Ultimate Guide to AI Agent Workforce 2026 - O-mega.ai](https://o-mega.ai/articles/openclaw-creating-the-ai-agent-workforce-ultimate-guide-2026) - Boost productivity in 2026 with OpenClaw AI agents automating real tasks across your favorite apps. ...

6. [OpenClaw Architecture Guide | High-Reliability AI Agent Framework](https://vertu.com/ai-tools/openclaw-clawdbot-architecture-engineering-reliable-and-controllable-ai-agents/) - Master OpenClaw's 6-stage pipeline. Use Lane Queues and Semantic Snapshots to eliminate state drift ...

7. [OpenClaw: The Architecture of the Personal AI Operating System](https://alphanometech.substack.com/p/openclaw-the-architecture-of-the) - OpenClaw (formerly known as Clawdbot or Moltbot) is an open-source â€œagentic runtimeâ€ designed to liv...

8. [OpenClawd Releases Major Platform Update as OpenClaw Surpasses React With 250,000 GitHub Stars](https://finance.yahoo.com/news/openclawd-releases-major-platform-openclaw-150000544.html) - The Clawdbot AI Assistant Now Has More GitHub Stars Than React. OpenClawd Wants to Make Sure You Can...

9. [nanobot: Ultra-Lightweight AI Assistant by HKUDS](https://nanobot.club) - nanobot is an ultra-lightweight personal AI assistant by HKUDS (HKU) with only ~4,000 lines of Pytho...

10. [OpenClaw instances open to the internet present ripe targets](https://www.theregister.com/2026/02/09/openclaw_instances_exposed_vibe_code/) - More than 135,000 OpenClaw instances exposed to internet in latest vibe-coded disaster ... OpenClaw ...

11. [Over 220,000 OpenClaw Instances Exposed to the Internet, Why ...](https://www.penligent.ai/hackinglabs/over-220000-openclaw-instances-exposed-to-the-internet-why-agent-runtimes-go-naked-at-scale/) - A fact-checked, engineering-first analysis of the OpenClaw internet exposure waveâ€”why reported count...

12. [OpenClaw Security Risks: From Vulnerabilities to Supply Chain Abuse](https://www.sangfor.com/blog/cybersecurity/openclaw-ai-agent-security-risks-2026) - In February 2026, SecurityScorecard reported observing 40,214 internet-exposed OpenClaw instances, n...

13. [The Ultimate Guide to OpenClaw Security Issues in 2026 - Skywork](https://skywork.ai/skypage/en/openclaw-security-issues/2037034777661227008) - On January 31, 2026, security firm Censys identified 21,639.00 exposed instances publicly accessible...

14. [NanoClaw Framework: Why Your AI Agent is Trying to Hack You](https://www.youtube.com/watch?v=i16R2wZe2xk) - In this video, we delve into the revolutionary NanoClaw framework that redefines AI agent security b...

15. [NanoClaw solves one of OpenClaw's biggest security issues](https://novalogiq.com/2026/02/11/nanoclaw-solves-one-of-openclaws-biggest-security-issues-and-its-already-powering-the-creators-biz/) - The rapid viral adoption of Austrian developer Peter Steinbergerâ€™s open source AI assistant OpenClaw...

16. [The Ultimate 2026 Guide to PicoClaw OpenClaw AI Agents](https://skywork.ai/skypage/en/ultimate-guide-picoclaws-openclaw-ai-agents/2037445873990635520) - It boasts over 247,000 GitHub stars and features a massive ecosystem of over 13,729.00 community ski...

17. [NullClaw AI Assistant Built with Zig, 0.1% Resources - LinkedIn](https://www.linkedin.com/posts/thetechfrontier_thetechfrontier-ziglang-aiagents-activity-7434203882769231872-JMTR) - While the industry has been racing to build more complex AI assistants, a project called NullClaw ju...

18. [ZeroClaw Review: OpenClaw's Speed Demon Rewrite in Rust](https://awesomeagents.ai/reviews/review-zeroclaw/) - ZeroClaw rewrites OpenClaw's core in Rust, delivering 14x faster skill execution, 90% lower memory u...

19. [Gavriel Cohen's Post - LinkedIn](https://www.linkedin.com/posts/gavrielco_two-huge-milestones-in-24-hours-nanoclaw-activity-7442256231559270400-OL2N) - Two HUGE milestones in 24 hours âœ¨ NanoClaw just crossed 25,000 GitHub stars. And we just became the ...

20. [NanoClaw](https://nemoclaw.bot/nanoclaw-secure-ai-agent.html) - NanoClaw is the container-native, secure sandbox AI agent variant providing OS-level isolation and R...

21. [NanoClaw solves one of OpenClaw's biggest security issues](https://news.ycombinator.com/item?id=46976845)

22. [nanobot: The Ultra-Lightweight OpenClaw - GitHub](https://github.com/HKUDS/nanobot) - nanobot is an ultra-lightweight personal AI assistant inspired by OpenClaw. âš¡ï¸ Delivers core agent f...

23. [Releases Â· HKUDS/nanobot - GitHub](https://github.com/HKUDS/nanobot/releases) - Reliability took center stage â€” A lot of this release is about making nanobot fail more gracefully. ...

24. [[Feature Request] Universal Tool Orchestrator: Parallel Execution ...](https://github.com/HKUDS/nanobot/issues/1378) - Description Â· Speed: Massive reduction in total execution time via parallelization. Â· Efficiency: Fe...

25. [Chao Huang on X: "nanobot hits 5k stars in just 3 days! That's wild!ðŸ¤¯Seems like there's real demand for ultra-lightweight OpenClaw. While OpenClaw requires 430,000 lines of code, nanobot delivers core functionality in just 4,000 lines of Python with 2-minute deployment. Our vision is simple: https://t.co/cRsFXIQkA3" / X](https://x.com/huang_chao4969/status/2019093499644440874)

26. [picoclaw/README.vi.md at main - GitHub](https://github.com/sipeed/picoclaw/blob/main/README.vi.md) - PicoClaw Ä‘Ã£ Ä‘áº¡t 25K Stars! 2026-03-09 v0.2.1 â€” Báº£n cáº­p nháº­t lá»›n nháº¥t tá»« trÆ°á»›c Ä‘áº¿n nay! Há»— trá»£ giao t...

27. [github.com/sipeed/picoclaw v0.2.4-0.20260319212038 ... - Libraries.io](https://libraries.io/go/github.com%2Fsipeed%2Fpicoclaw) - Tiny, Fast, and Deployable anywhere â€” automate the mundane, unleash your creativity

28. [README.md - sipeed/picoclaw](https://github.com/sipeed/picoclaw/blob/main/README.md) - Tiny, Fast, and Deployable anywhere â€” automate the mundane, unleash your creativity - sipeed/picocla...

29. [GitHub - sipeed/picoclaw: Tiny, Fast, and Deployable anywhere](https://github.com/sipeed/picoclaw) - Tiny, Fast, and Deployable anywhere â€” automate the mundane, unleash your creativity - sipeed/picocla...

30. [PicoClaw and OpenClaw: The Evolution of Autonomous AI Agents](https://skywork.ai/slide/en/pclaw-autonomous-ai-agents-2037454791090454528) - Presentation Contents: Exploring **PicoClaw** & **OpenClaw**. A Comprehensive Roadmap to the Future ...

31. [PicoClaw Roadmap: March 2026 (Week 2) Â· Issue #988 - GitHub](https://github.com/sipeed/picoclaw/issues/988) - ðŸ–¥ï¸ 1. WebUI Enhancements Â· 2. Agent Refactor (Kick-off) Â· 3. Security Â· ðŸ‘ï¸ 4. Multimodal LLM Support...

32. [ZeroClaw | Autonomous Rust AI Agent Framework](https://zeroclaw.bot) - Official overview of ZeroClaw, a fast and secure Rust-based autonomous AI agent framework.

33. [OpenClaw Alternatives: NanoClaw, ZeroClaw, Moltis, and Every ...](https://www.aimagicx.com/blog/openclaw-alternatives-comparison-2026) - We compare NanoClaw, ZeroClaw, Moltis, PicoClaw, Nanobot, NullClaw, and more â€” covering architecture...

34. [Getting Started](https://dev.to/brooks_wilson_36fbefbbae4/zeroclaw-a-lightweight-secure-rust-agent-runtime-redefining-openclaw-infrastructure-2cl0) - ZeroClaw: Rebuilding AI Agent Infrastructure from the Ground Up in Rust github repo...

35. [ZeroClaw: The Ultra-Lightweight AI Agent Runtime | Rust-Based](https://zeroclaw.net) - ZeroClaw is a high-performance, Rust-based AI agent runtime. It offers 400x faster startup, 99% lowe...

36. [OpenClaw vs ZeroClaw: Definitive AI Agent Framework Comparison](https://sparkco.ai/blog/openclaw-vs-zeroclaw-which-ai-agent-framework-should-you-choose-in-2026) - Data-driven, feature-by-feature comparison of OpenClaw and ZeroClaw for technical decision-makers. C...

37. [ZeroClaw â€” Rust AI Agent Runtime | Releases & Guides](https://zeroclaw.space) - Track ZeroClaw releases, install guides and comparisons. A Rust-based, open-source AI agent runtime ...

38. [ZeroClaw vs OpenClaw vs PicoClaw: AI Agent Comparison (Tested)](https://zeroclaw.net/zeroclaw-vs-openclaw-vs-picoclaw) - Compare the top AI agent frameworks of 2026: ZeroClaw, OpenClaw, and PicoClaw. Learn about their arc...

39. [Meet NullClaw: The 678 KB Zig AI Agent Framework Running on 1 ...](https://www.marktechpost.com/2026/03/02/meet-nullclaw-the-678-kb-zig-ai-agent-framework-running-on-1-mb-ram-and-booting-in-two-milliseconds/) - Meet NullClaw: The 678 KB Zig AI Agent Framework Running on 1 MB RAM and Booting in Two Milliseconds...

40. [nullclaw/AGENTS.md at main - GitHub](https://github.com/nullclaw/nullclaw/blob/main/AGENTS.md) - nullclaw is a Zig-first autonomous AI assistant runtime optimized for: minimal binary size (target: ...

41. [Claw Owners Earn, Business Owners Get More Done vs NullClaw](https://aiagentstore.ai/compare-ai-agents/claw-earn-claw-owners-earn-business-owners-get-more-done-vs-nullclaw) - It supports 22+ model providers, 18 communication channels, hybrid memory, streaming, voice, and mul...

42. [NVIDIA NemoClaw: How It Works, Use Cases & Features ...](https://www.secondtalent.com/resources/nvidia-nemoclaw/) - OpenClaw became the fastest-growing open-source project in history. Over 4,600 GitHub stars. Thousan...

43. [Nvidia GTC 2026: NemoClaw and Enterprise Agentic AI](https://www.digitalapplied.com/blog/nvidia-gtc-2026-nemoclaw-openclaw-enterprise-agentic-ai) - GTC 2026 complete recap: Vera Rubin platform, NemoClaw enterprise agents, Nemotron Coalition, Dynamo...

44. [NVIDIA NemoClaw Explained: Open Source AI Agent Platform for ...](https://www.mayhemcode.com/2026/03/nvidia-nemoclaw-explained-open-source.html) - NVIDIA's NemoClaw adds enterprise security and privacy to OpenClaw AI agents in a single command. He...

45. [NVIDIA NemoClaw: Enterprise AI Agents Without Lock-In](https://awesomeagents.ai/news/nvidia-nemoclaw-enterprise-ai-agents/) - NVIDIA is preparing to launch NemoClaw, an open-source enterprise AI agent platform that runs on any...

46. [NemoClaw: What It Is, How It Works, and Alternatives (2026 Guide)](https://www.nemoclaw.so) - NemoClaw is NVIDIA's open-source AI agent platform for enterprises, unveiled at GTC 2026. Learn what...

47. [NemoClaw â€” NVIDIA NemoClaw AI Agent Platform | Announced at ...](https://nemo-claw.net) - NVIDIA NemoClaw (Nemo Claw AI) is the open-source AI agent platform announced at NVIDIA GTC 2026 key...

48. [Is NemoClaw Free? Complete Pricing Breakdown (2026)](https://sphinxagent.com/blog/is-nemoclaw-free-pricing.html) - Is NemoClaw free? Yes and no. Software is free, but hardware costs $2K-$50K. Complete pricing guide ...

49. [NousResearch's Self-Improving Open-Source Agent Framework](https://clauday.com/article/367ef753-e84f-4d23-ae7f-e87d4e0a55a9) - Nous Research has released Hermes Agent, an open-source AI agent framework that learns and improves ...

50. [Hermes Agent Documentation - Nous Research](https://hermes-agent.nousresearch.com/docs/) - The self-improving AI agent built by Nous Research. A built-in learning loop that creates skills fro...

51. [Hermes Agent: what Nous Research built - CrabTalk](https://www.crabtalk.ai/blog/hermes-agent-survey) - We examined Hermes Agent's architecture â€” from Atropos RL training to persistent skill documents. He...

52. [Nous Research has introduced Hermes Agent, a highly capable ...](https://x.com/WesRoth/status/2026960434650075512)

53. [Agent Loop Internals | Hermes Agent - Nous Research](https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop/) - Detailed walkthrough of AIAgent execution, API modes, tools, callbacks, and fallback behavior

54. [Security | Hermes Agent - Nous Research](https://hermes-agent.nousresearch.com/docs/user-guide/security/) - Security model, dangerous command approval, user authorization, container isolation, and production ...

55. [OpenClaw vs Hermes Agent Security: How Each Open-Source AI ...](https://getclaw.sh/blog/openclaw-vs-hermes-agent-security-model-comparison-2026) - A technical blueprint comparing OpenClaw and Hermes Agent security architectures, showing task flow,...

56. [Smolagents has now reached 15k GitHub stars : it's one of the fastest-growing agentic frameworks ever! | Aymeric Roucher](https://www.linkedin.com/posts/a-roucher_smolagents-has-now-reached-15k-github-stars-activity-7309986405118586881-Px0X) - Smolagents has now reached 15k GitHub stars : it's one of the fastest-growing agentic frameworks eve...

57. [agents-course/units/en/unit2/smolagents/code_agents.mdx at main](https://github.com/huggingface/agents-course/blob/main/units/en/unit2/smolagents/code_agents.mdx) - Code agents are the default agent type in smolagents . They generate Python tool calls to perform ac...

58. [Smolagents vs LangGraph: Which One's Easier to Build and Run AI ...](https://www.zenml.io/blog/smolagents-vs-langgraph) - In this Smolagents vs LangGraph, we explain the difference between the two and conclude which one is...

59. [smolagents - Hugging Face](https://huggingface.co/docs/smolagents/index) - Weâ€™re on a journey to advance and democratize artificial intelligence through open source and open s...

60. [Smolagents: Hugging Face's New Agentic Framework](https://www.laloadrianmorales.com/blog/smolagents-hugging-faces-new-agentic-framework/) - Smolagents: Hugging Faceâ€™s New Agentic Framework Introduction Itâ€™s often said that some years bring ...

61. [Hugging Face's Minimalist Framework for Building Powerful AI Agents](https://joshuaberkowitz.us/blog/github-repos-8/smolagents-hugging-face-s-minimalist-framework-for-building-powerful-ai-agents-2068) - Simplicity Meets Power in AI

62. [ALucek/smolagents-guide: A walk through HuggingFace ... - GitHub](https://github.com/ALucek/smolagents-guide) - From the developers at Hugging Face comes smolagents, a powerful yet streamlined framework for build...

63. [CodeAgents + Structure: A Better Way to Execute Actions](https://huggingface.co/blog/structured-codeagent) - Our implementation of CodeAgent in smolagents extracts Python code from the LLM output, which can fa...

64. [ENH: Add lifecycle hooks for CodeAgent execution flow Â· Issue #1883](https://github.com/huggingface/smolagents/issues/1883) - I'm working on a multi-agent computer use system for accessibility tech, and I need to execute callb...

65. [Agent will get stuck after a few iterations if used in a loop Â· Issue #582](https://github.com/huggingface/smolagents/issues/582) - I am trying to run an agent in a for loop, and after just a few iterations it gets stuck for no reas...

66. [LightAgent: Lightweight Agentic AI Framework](https://www.emergentmind.com/papers/2509.09292) - LightAgent is an open-source framework for multi-agent systems leveraging LLMs with integrated memor...

67. [LightAgent - PyPI](https://pypi.org/project/LightAgent/) - LightAgent: Lightweight AI agent framework with memory, tools & tree-of-thought. Supports multi-agen...

68. [LightAgent: Production-level Open-source Agentic AI ...](https://arxiv.org/html/2509.09292v1)

69. [GitHub - wxai-space/LightAgent: LightAgent: Lightweight AI agent framework with memory, tools & tree-of-thought. Supports multi-agent collaboration, self-learning, and major LLMs (OpenAI/DeepSeek/Qwen). Open-source with MCP/SSE protocol integration.](https://github.com/wxai-space/LightAgent) - **LightAgent** is an extremely lightweight active Agentic Framework with memory, tools , and a Tree ...

70. [GitHub - wxai-space/LightAgent: **LightAgent** is an extremely lightweight active Agentic Framework with memory, tools , and a Tree of Thought (`ToT`). It supports swarm-like multi-agent collaboration, automated tool generation, and agent assessment, with underlying model support for OpenAI, ChatGLM, Baichuan, DeepSeek, Qwen](https://github.com/wxai-space/LightAgent/) - **LightAgent** is an extremely lightweight active Agentic Framework with memory, tools , and a Tree ...

71. [Built with Rust: MicroClaw as a Multi-Channel Agent Runtime](https://microclaw.ai/blog/built-with-rust-microclaw-runtime/) - It supports Telegram, Discord, Slack, Feishu/Lark, IRC, and Web through adapters, while keeping one ...

72. [GitHub Open Source Weekly 2026-03-25 - Shareuhack](https://www.shareuhack.com/en/posts/github-trending-weekly-2026-03-25) - This week's +21,490 stars (104K total) briefly overtook superpowers, signaling that developers want ...
