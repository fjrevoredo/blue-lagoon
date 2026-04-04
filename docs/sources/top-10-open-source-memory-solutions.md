# Top 10 Open Source LLM Agent Memory Solutions

**Metadata for LLM Consumers**
- Document type: Comparative technical research report
- Primary audience: AI engineers, ML practitioners, LLM agent developers
- Coverage scope: Open-source LLM agent memory frameworks as of Q1/Q2 2026
- Key evaluation dimensions: Feature set, benchmark performance, community adoption, hardware requirements, scalability, long-term stability
- Canonical benchmarks cited: LongMemEval, LoCoMo (Long Conversation Memory), Deep Memory Retrieval (DMR)
- Recommended read mode: Structured reference â€” use section headers as semantic anchors

***

## Executive Summary

LLM agents are stateless by default: each session starts from zero. Agent memory frameworks solve this by extracting, storing, and retrieving knowledge across sessions. As of 2026, the field has converged on a core architectural consensus: hybrid storage (vector + graph), temporal awareness, and active memory consolidation are the three pillars of high-performing memory systems. Pure vector search plateaus at 60â€“70% retrieval accuracy at scale, while graph-augmented hybrid approaches consistently score 70â€“90%+. Long-term memory degradation â€” caused by semantic drift, noise accumulation, and unconstrained memory growth â€” is a key unsolved production challenge that separates mature solutions from experimental ones.[^1][^2][^3][^4]

This report covers **10 open-source solutions**, comparing them across six critical dimensions, and concludes with a top-3 recommendation with rationale tailored for production deployment decisions.

***

## Taxonomy of Agent Memory Architectures

Understanding the architectural families is prerequisite to evaluating individual tools.

| Architecture Family | Data Model | Strengths | Known Failure Modes |
|---|---|---|---|
| Plain vector RAG | Embedding vectors (ANN) | Fast (10â€“50ms), simple | Semantic drift, no temporal/relational context[^5] |
| Tiered vector (OS-inspired) | Working set + vector archive | Bounded context, long conversations | Paging policy errors, per-agent divergence[^5] |
| Temporal Knowledge Graph (TKG) | Temporal KG + vector hybrid | Strong temporal/cross-session reasoning | Stale edges, high ingestion cost[^5][^1] |
| Knowledge Graph RAG (GraphRAG) | KG + community hierarchies | Multi-doc, multi-hop questions | Over-summarization, high build cost[^5] |
| Hierarchical OS Memory | Short/mid/long-term tiers + heat-based eviction | Personalization, coherent long conversations | Boundary errors, consolidation bias[^6] |
| Multi-strategy Hybrid | Semantic + BM25 + graph + temporal + reranking | Highest recall, production-grade | Higher retrieval latency (100â€“600ms)[^7] |

***

## The Long-Term Degradation Problem

This is the most underappreciated challenge in production memory deployment.

**Semantic drift** occurs when knowledge degrades through iterative summarization â€” stored facts lose precision as the agent's extraction pipeline repeatedly re-processes and overwrites entries. Formal analysis shows that unconstrained autonomy in memory writing is the primary catalyst for both semantic drift and catastrophic forgetting. Solutions include immutable episodic logs paired with mutable active graphs, enabling rollback to raw interaction traces when severe behavioral degradation occurs.[^3]

**Agent drift** describes the progressive degradation of agent decision quality and coherence over extended interactions. Research demonstrates that all agent stability indicators decline roughly linearly through the first 300 interactions, after which accumulated drift becomes self-reinforcing. Three mitigation strategies show empirical efficacy: episodic memory consolidation, drift-aware routing, and adaptive behavioral anchoring â€” with the combined approach achieving 94.7% drift reduction.[^4]

**Vector search degradation** is architectural: retrieval cost scales linearly with corpus size, semantic proximity becomes meaningless in heterogeneous datasets, and the same facts are frequently stored multiple times without consolidation, making relevance degrade with noise accumulation.[^2]

Frameworks that address these issues natively (through memory consolidation pipelines, consolidation equilibrium control, and explicit temporal versioning) substantially outperform those that treat memory as a simple append-only store.

***

## The 10 Frameworks

### 1. Mem0

**License:** Apache 2.0 | **GitHub Stars:** ~48K[^8] | **Funding:** $24M Series A (Basis Set Ventures, Oct 2025)[^7]

**Architecture:** Dual-store combining a vector database and an optional knowledge graph. An LLM-powered extraction pipeline converts conversation messages into atomic memory facts, scoped to users, sessions, or agents. Supports Qdrant, Chroma, Milvus, pgvector, and Redis as vector backends. Graph memory (Mem0g variant) adds entity-relationship links for multi-hop relational queries, though this is paywalled on the managed platform.[^8][^7]

**Memory Types:** User-level, session-level, and agent-level semantic memory. Short-term in-session buffering via conversation history injection.[^9]

**Retrieval:** Semantic vector search (baseline); hybrid vector + graph traversal on Pro/self-hosted graph variant. Achieves sub-1.5s p95 latency even with extensive conversation histories. A benchmark paper reports 91% lower p95 latency and 90% token savings vs. full-context approaches.[^10][^11]

**Benchmark Performance (LLM-evaluated):**
- LoCoMo (self-reported): ~66â€“68.5% J-score[^12]
- LongMemEval (independent evaluation): 49.0%[^8]
- Note: Mem0's own paper claims SOTA on LOCOMO, but independent replications and competitor analyses dispute these claims[^13][^11][^14]

**Hardware Requirements:** Minimum 4 GB RAM; vector DB backend required (Qdrant/pgvector typical); no GPU needed for the memory layer itself; LLM API calls external.[^15]

**Scalability:** Multi-tenant by design; each memory store is scoped by user/session/agent ID. Managed platform handles thousands of concurrent agents. Aurora PostgreSQL integration supports up to 256 TB storage with 15 read replicas. Self-hosted scales with the chosen vector backend.[^16]

**Long-Term Performance:** Memory consolidation handles up to 10K memories per user with sub-100ms updates. Risk of semantic drift exists due to LLM-delegated memory decisions; the LLM decides what is "important," which may miss nuanced information. No built-in memory decay or consolidation equilibrium control in the open-source tier.[^17][^8]

**Ecosystem:** Python and TypeScript SDKs; integrations with LangChain, CrewAI, LlamaIndex, and more; OpenMemory MCP server for local-first deployment; SOC 2 and HIPAA compliance on managed platform.[^7][^18]

**Strengths:** Largest community, fastest adoption, framework-agnostic, battle-tested managed service, strong documentation.

**Weaknesses:** Graph features paywalled; independent benchmarks score significantly lower than self-reported numbers; architectural simplicity can lead to missing relational/temporal context.

***

### 2. Letta (formerly MemGPT)

**License:** Apache 2.0 | **GitHub Stars:** ~21K[^8] | **Funding:** $10M seed, Felicis Ventures[^7]

**Architecture:** OS-inspired tiered memory â€” an agent runtime (not just a library) where agents actively manage their own memory. Three tiers: Core Memory (always in context window, analogous to RAM), Recall Memory (searchable conversation history), and Archival Memory (long-term cold storage backed by vector DB).[^19][^20]

**Memory Types:** All memory types through tiered architecture. Agents self-edit memory blocks using explicit tool calls to decide what to keep, archive, or delete.[^7]

**Retrieval:** Agent-driven via tool calls against memory tiers. Non-deterministic â€” quality depends on the underlying LLM's judgment about memory operations. Every memory operation costs inference tokens.[^8]

**Benchmark Performance:**
- LoCoMo: ~74â€“83% (varying reports)[^21][^12]
- DMR: 93.4% (the benchmark Letta/MemGPT established)[^22]
- No published LongMemEval results[^8]

**Hardware Requirements:** Docker deployment; 512 MB minimum RAM (1 GB+ recommended for production). LanceDB as default vector backend (file-based). Production deployments typically use PostgreSQL or Aurora for persistence.[^23][^16]

**Scalability:** Agent sharding by persona; per-agent isolation. AWS Aurora PostgreSQL integration scales storage from 10 GiB to 256 TB with no downtime. Designed as a cloud-native framework.[^16]

**Long-Term Performance:** Self-editing memory is maximally adaptive but non-deterministic and model-dependent. Reported stability issues with smaller or locally run models. Memory management policy is embedded in the LLM prompt, meaning weaker models produce inconsistent memory behavior over time. No explicit drift mitigation beyond agent reasoning.[^24][^1]

**Ecosystem:** Agent Development Environment (ADE) for visual inspection; Python SDK; model-agnostic (OpenAI, Anthropic, Ollama, Vertex AI); MCP server support.[^25]

**Strengths:** Innovative self-editing architecture; strong research foundation (peer-reviewed MemGPT paper); unique ADE tooling; excellent for long-running personalized agents.

**Weaknesses:** Full runtime commitment (not just a memory layer); steeper learning curve; stability issues with weaker models; pivoted toward coding agent use cases which may narrow focus.[^24]

***

### 3. Graphiti / Zep

**License:** Graphiti: MIT open source | Zep CE: deprecated | **GitHub Stars (Graphiti):** ~24K[^7] | **Funding:** Zep AI (YC W24)

**Architecture:** Temporal Knowledge Graph engine. Episodes (text or structured JSON) are ingested and automatically decomposed into entities, edges, and temporal attributes. Every fact carries validity windows (when it became true, when superseded), indexed via interval trees for efficient historical queries. Supports Neo4j, FalkorDB, and Kuzu as graph backends. Recent efficiency improvements: 50% lower token usage via entropy-gated fuzzy matching, MinHash, LSH, and Jaccard similarity.[^26][^27][^7]

**Memory Types:** Episodic (raw interaction log), semantic (extracted entities/relationships), temporal (validity-windowed facts). Best in class for tracking fact evolution over time.

**Retrieval:** Multi-strategy: semantic search, BM25, graph traversal â€” all pre-computed at write time for fast reads. No LLM calls at query time. P95 retrieval latency ~300ms on Zep Cloud; 0.632s p95 search latency in corrected benchmark conditions.[^28][^1][^13]

**Benchmark Performance:**
- DMR: 94.8% (vs. 93.4% MemGPT baseline)[^22]
- LoCoMo (corrected): 75.14% Â± 0.17 J-score[^13]
- LongMemEval: 71.2% (GPT-4o)[^7]
- LongMemEval temporal accuracy: up to 18.5% improvement, 90% lower latency vs. baselines[^22]

**Hardware Requirements:** Graph database required â€” Neo4j (~4 GB+ RAM recommended for production), or FalkorDB/Kuzu as lighter alternatives. CPU-only for the memory layer; LLM for ingestion-time entity extraction.

**Scalability:** Each user has an isolated graph; graphs are processed concurrently at scale. Graph-native horizontal scaling. Enterprise deployments via Zep Cloud (Community Edition deprecated; self-hosting now means running raw Graphiti).[^29][^7]

**Long-Term Performance:** Strongest temporal awareness of any framework â€” explicit validity windows prevent stale facts from contaminating current state. Ingestion for large corpora can take hours due to multiple LLM calls for entity extraction, resolution, and relationship inference. Zep's paper acknowledges that immediate memory retrieval after ingestion may be unreliable due to asynchronous background processing. Temporal versioning directly mitigates semantic drift.[^30][^1][^7]

**Ecosystem:** Python, TypeScript, Go SDKs; MCP Server 1.0 (hundreds of thousands of weekly users); integrations with Claude Desktop, Cursor, LangChain, LangGraph; AWS, Microsoft, Neo4j, FalkorDB as contributors.[^27][^26]

**Strengths:** Best temporal reasoning, richest relational representation, pre-computed retrieval, production-validated via Zep platform.

**Weaknesses:** Complex infrastructure (requires graph DB); slow ingestion for large datasets; Zep CE deprecated limits full self-hosting; benchmark numbers contested by competitors.

***

### 4. Cognee

**License:** Apache 2.0 (open core) | **GitHub Stars:** 14.7K[^31] | **Funding:** â‚¬7.5M (~$8.1M) seed

**Architecture:** ECL (Extract, Cognify, Load) pipeline that processes data through three phases: Extract (from 30+ connectors), Cognify (entity extraction + relationship resolution + embedding generation), and Load (parallel storage in vector and graph databases). Default local stack: SQLite (metadata), LanceDB (vectors), Kuzu (knowledge graph) â€” zero external infrastructure required.[^32][^33][^34]

**Memory Types:** Institutional knowledge (document-centric), semantic entities and relationships, cross-agent shared memory. Includes an adaptive memory weighting system where frequently accessed connections strengthen over time via the `memify()` pipeline.[^35]

**Retrieval:** Hybrid graph traversal + vector similarity in a single query; temporal filtering supported. Multi-hop reasoning via graph traversal (e.g., HotPotQA improvements reported).[^36]

**Benchmark Performance:** No published LongMemEval/LoCoMo numbers on official docs. Community reports show the semantic layer approach surpasses the 60â€“70% vector search plateau. Production data from 200K+ pipeline runs.[^2]

**Hardware Requirements:** Fully local â€” no GPU required; any modern CPU; zero external dependencies in dev mode (SQLite/LanceDB/Kuzu); production uses PostgreSQL 12+ with pgvector, optional Neo4j.[^34][^37]

**Scalability:** Local-first design scales to hosted Cogwit service without redesign. Kuzu can scale to billion-node graphs on commodity hardware. PostgreSQL backend for production multi-tenant deployments.[^36][^34]

**Long-Term Performance:** The `memify()` pipeline uses RL-inspired optimization to strengthen useful pathways, remove obsolete nodes, and auto-tune based on real usage. Hierarchical memory layers (raw episodes â†’ session traces â†’ meta-nodes â†’ shared world model) address the consolidation equilibrium problem that causes pure-vector systems to degrade. Graduated GitHub's Secure Open Source Program.[^38][^35][^2]

**Ecosystem:** Python-only (no TypeScript/Go SDKs); MCP integration; 30+ data connectors including PDFs, images, audio, code; supports OpenAI, Claude, and local models via LM Studio.[^39][^7]

**Strengths:** Best zero-infrastructure local deployment; multimodal ingestion; knowledge graph available at all tiers (no paywall); strong consolidation/drift-mitigation architecture.

**Weaknesses:** Python-only; smaller community; managed cloud is newer; no published standardized benchmarks.

***

### 5. LangMem (LangChain Memory SDK)

**License:** MIT | **GitHub Stars:** ~1.3K[^7] | **Maintainer:** LangChain

**Architecture:** Memory primitives tightly integrated with LangGraph's persistent store. Three components: Core Memory API (storage-agnostic), Memory Tools (agent-accessible tools for recording/searching memories in the "hot path"), and a Background Memory Manager (async extraction and consolidation outside the conversation flow).[^40][^41]

**Memory Types:** Semantic (facts), episodic (experiences), procedural (agent behavior patterns via prompt optimization).[^41]

**Retrieval:** Single-strategy vector similarity only; no knowledge graph or entity extraction. Direct key access or semantic search. Namespace/shard support to narrow search space.[^42][^40][^7]

**Benchmark Performance:** No published LongMemEval/LoCoMo scores.

**Hardware Requirements:** Minimal â€” InMemoryStore for dev, AsyncPostgresStore for production. No GPU required; scales with the chosen storage backend.[^40][^42]

**Scalability:** Scales via LangGraph's storage layer (Postgres, Redis). Risk: without pruning/compression policies, retrieval quality degrades as memory grows to thousands of entries.[^42]

**Long-Term Performance:** Vector search only means the standard vector degradation problems apply at scale. Background memory manager performs periodic consolidation, but no graph-based relational maintenance. LangMem explicitly documents that unchecked memory accumulation degrades retrieval quality and recommends pruning policies and time-based decay.[^42]

**Ecosystem:** Python-only; deep LangGraph integration; prompt optimization feature unique for refining agent system prompts from conversation data.[^7]

**Strengths:** Free, zero additional infrastructure, unique prompt optimization, native LangGraph integration.

**Weaknesses:** Severe framework lock-in; no knowledge graph; slowest retrieval of dedicated tools (50s+ p95 latency reported in some benchmarks); development cadence has slowed.[^10][^7]

***

### 6. LlamaIndex Memory

**License:** MIT | **GitHub Stars:** Part of ~48K LlamaIndex total[^7] | **Maintainer:** LlamaIndex/Jerry Liu

**Architecture:** Composable buffer-based memory modules: `ChatMemoryBuffer` (FIFO with configurable token limits), `ChatSummaryMemoryBuffer` (auto-summarizes on overflow), `VectorMemory` (semantic search over history), and the newer pluggable `Memory` class with `FactExtractionMemoryBlock` backed by SQLite for cross-session persistence.[^43][^44]

**Memory Types:** Short-term conversation context; static profiles; dynamic fact extraction (newer API). Session-scoped by default; cross-session requires the newer `Memory` class with `async_database_uri`.[^43]

**Retrieval:** Vector similarity only; no knowledge graph or entity resolution. Framework coupling means migrating to a different agent framework loses the memory layer.[^45]

**Benchmark Performance:** No published memory-specific benchmarks.

**Hardware Requirements:** Minimal â€” SQLite (file-based) by default for local; any supported vector backend for production. No GPU required.

**Scalability:** Scales via LlamaCloud for managed infrastructure. Local SQLite is not horizontally scalable; production requires configuring an external database.

**Long-Term Performance:** Session-scoped buffers reset on restart (the fundamental limitation). Newer `Memory` class adds cross-session persistence but lacks synthesis, graph traversal, or entity resolution for complex queries. No built-in consolidation or drift mitigation.[^45]

**Ecosystem:** Massive â€” full LlamaIndex data framework (connectors, parsers, query engines); separate KG capabilities not integrated with memory modules.[^7]

**Strengths:** Mature ecosystem, well-documented, composable primitives, free.

**Weaknesses:** Primarily session-scoped; no entity extraction or graph in memory modules; tightly coupled to LlamaIndex.

***

### 7. MemoryOS

**License:** Open source (Apache-compatible) | **GitHub Stars:** Growing (academic release 2025) | **Paper:** EMNLP 2025 Oral[^46][^47]

**Architecture:** Memory Operating System inspired directly by OS memory management principles. Four modules: Storage (three-tier hierarchy), Updating (FIFO â†’ summary promotion â†’ heat-based eviction), Retrieval (semantic + metadata), and Generation (compatible with any LLM). Short-term to mid-term updates use dialogue-chain FIFO; mid-term to long-term uses segmented page organization. Includes a persona module that captures evolving user preferences via personalized trait extraction.[^6]

**Memory Types:** Short-term (chat buffer), mid-term (summaries, recent context), long-term (personal knowledge + persona). Heat-driven eviction dynamically prioritizes high-value information.[^48]

**Retrieval:** Semantic retrieval with heat-weighted scoring. The tiered architecture means frequently-accessed memories are physically closer (faster to access) and less-accessed memories are promoted to cold storage.

**Benchmark Performance:**
- LoCoMo with GPT-4o-mini: +49.11% improvement in F1, +46.18% in BLEU-1 over baseline[^46]
- Efficiency comparable to Mem0; substantially faster than A-mem[^49]

**Hardware Requirements:** Standard LLM inference hardware; no GPU required for the memory layer itself; MCP server deployment on standard Docker host.[^50]

**Scalability:** Designed for single-agent personalization use cases (e.g., personal AI assistants). Multi-tenant scaling not a primary design goal. MCP server enables integration with multiple LLM clients.[^50]

**Long-Term Performance:** Heat-based eviction is the core mechanism for preventing unbounded memory growth and quality degradation. Memories that are not accessed "cool off" and are eventually evicted, preventing noise accumulation. However, this also risks losing infrequently accessed but important facts. Persona module adapts to long-term behavioral drift.[^6]

**Ecosystem:** Python; simple 40-line demo; OpenAI/Deepseek/Qwen/local vLLM support; MCP server (MemoryOS-MCP).[^48][^50]

**Strengths:** Solid academic backing (EMNLP 2025 Oral); principled heat-driven eviction; best fit for personal assistant personalization; lightweight.

**Weaknesses:** Primarily academic; smaller production footprint; no graph capabilities; enterprise deployment not validated.

***

### 8. Memoripy

**License:** Open source (MIT-compatible) | **GitHub Stars:** ~1.5K

**Architecture:** Short-term and long-term memory with semantic clustering for grouping similar memories. Incorporates memory decay â€” less significant memories diminish while frequently accessed ones remain prominent, simulating human forgetting curves. Local-first storage designed to avoid external API calls.[^51][^52]

**Memory Types:** Short-term (recent interactions), long-term (persistent, semantically clustered). Decay model applied to long-term storage.[^51]

**Retrieval:** Semantic clustering retrieval; memories weighted by recency and access frequency.

**Benchmark Performance:** No published standardized benchmarks.

**Hardware Requirements:** CPU-only for the memory layer; local storage; supports Ollama and OpenAI for the LLM component.[^25][^51]

**Scalability:** Designed for single-agent or small-scale use cases; not designed for multi-tenant enterprise deployment.

**Long-Term Performance:** Memory decay is the primary anti-degradation mechanism â€” the system explicitly models forgetting to prevent noise accumulation. However, the decay model may not account for contextual importance (an infrequently accessed but critical fact may decay inappropriately).

**Ecosystem:** Python; Ollama + OpenAI + OpenRouter support; minimal dependencies.[^51]

**Strengths:** Human-inspired memory decay; fully local/offline; minimal infrastructure; good for privacy-sensitive deployments.

**Weaknesses:** Small community; no graph capabilities; limited scalability; no published benchmarks.

***

### 9. Hindsight (by Vectorize.io)

**License:** MIT | **GitHub Stars:** ~4K (rapidly growing)[^7] | **Funding:** $3.5M (April 2024)

**Architecture:** Multi-strategy hybrid retrieval with cross-encoder reranking. Four retrieval strategies run in parallel: semantic search (embeddings), BM25 keyword matching, entity graph traversal, and temporal filtering. Fact extraction, entity resolution, and knowledge graph construction happen at write time (read-optimized). Core synthesis operation (`reflect`) passes retrieved facts to an LLM for cross-memory reasoning.[^53][^7]

**Memory Types:** Personalization + institutional knowledge. Facts, entities, relationships extracted automatically; episodic memories preserved.

**Retrieval:** Multi-strategy parallel retrieval (100â€“600ms); `reflect()` synthesis adds 800â€“3000ms (optional LLM call).[^7]

**Benchmark Performance:**
- LongMemEval: 91.4% â€” highest published score on this benchmark[^53][^7]

**Hardware Requirements:** Embedded PostgreSQL + pgvector (Docker, ~2 GB+ RAM recommended). No GPU required. Single Docker command deployment.[^54]

**Scalability:** PostgreSQL-backed, horizontally scalable. Managed cloud offering also available. Designed for production from day one.

**Long-Term Performance:** Read-optimized architecture (heavy lifting at write time) means retrieval quality does not degrade with corpus growth in the same way pure vector systems do. Multi-strategy retrieval with cross-encoder reranking mitigates single-strategy failure modes. Entity resolution prevents duplicate fact storage.[^7]

**Ecosystem:** Python, TypeScript, Go SDKs; CrewAI, Pydantic AI, LiteLLM integrations; MCP-first design; Ollama support for fully local deployments.[^7]

**Strengths:** Highest LongMemEval score; multi-strategy retrieval addresses single-strategy failure modes; MIT license with no feature gating; `reflect()` enables synthesis not just retrieval.

**Weaknesses:** Newer project with smaller community; `reflect()` adds latency; community and production track record still developing.

***

### 10. Microsoft GraphRAG

**License:** MIT | **GitHub Stars:** ~24K (microsoft/graphrag) | **Maintainer:** Microsoft Research

**Architecture:** Knowledge Graph RAG via hierarchical community detection (Hierarchical Leiden algorithm). Extracts entities and relationships from source documents, builds a KG, runs community detection to produce multi-level summaries, and uses them at retrieval time. Optimized for large document corpora rather than conversational memory.[^5]

**Memory Types:** Institutional/document knowledge. Community-level and entity-level summaries. Not designed for session-level conversational memory.

**Retrieval:** Community identification (vector search over summaries) + local graph traversal. Two modes: Global (uses all communities for comprehensive answers) and Local (targeted entity-level retrieval). Query latency competitive for large corpora due to summary-based retrieval.[^5]

**Benchmark Performance:** Outperforms naive vector RAG on multi-document, multi-hop questions. No published LongMemEval/LoCoMo scores (different use case target).

**Hardware Requirements:** Higher than conversational memory frameworks â€” graph construction and community detection require significant compute. LLM calls for entity extraction and community summarization. Storage for the KG and summaries.

**Scalability:** Designed for large-scale document corpora. Indexing is compute-intensive but done offline. Query time scales with community structure, not raw document count.[^5]

**Long-Term Performance:** Static in nature â€” the graph is built from a fixed corpus and does not update dynamically from new interactions. Not suitable as a real-time conversational memory layer. Best as a one-time indexed knowledge base.

**Ecosystem:** Python; integrations with Azure AI; widely used for enterprise RAG; Prompt Flow compatible.

**Strengths:** Best for large static knowledge bases; global summarization enables questions that span an entire corpus; strong Microsoft backing.

**Weaknesses:** Not designed for dynamic conversational memory; high indexing cost; static graph degrades if underlying data changes; primarily a RAG framework, not an agent memory layer.

***

## Comprehensive Feature Matrix

| Framework | License | Stars | Memory Types | Graph | Temporal | Multi-Strategy Retrieval | Consolidation/Drift Mitigation | Python SDK | TypeScript SDK | Self-Host | Min RAM | GPU Required |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Mem0** | Apache 2.0 | ~48K | Semantic, Episodic | Pro only | No | Partial (Pro) | LLM-delegated | âœ… | âœ… | âœ… | 4 GB | No |
| **Letta** | Apache 2.0 | ~21K | All (tiered) | Via archival | Limited | No | Agent self-editing | âœ… | No | âœ… | 512 MB | No |
| **Graphiti** | MIT | ~24K | Episodic, Semantic, Temporal | âœ… | âœ… | Partial | Temporal versioning | âœ… | âœ… | âœ… | 4 GB+ | No |
| **Cognee** | Apache 2.0 | 14.7K | Semantic, Institutional | âœ… | Partial | Partial | memify() RL-based | âœ… | No | âœ… | ~512 MB | No |
| **LangMem** | MIT | ~1.3K | Semantic, Episodic, Procedural | No | No | No | Background consolidation | âœ… | No | âœ… | Minimal | No |
| **LlamaIndex Memory** | MIT | ~48K* | Short-term, Static, Facts | No | No | No | Summary buffer only | âœ… | No | âœ… | Minimal | No |
| **MemoryOS** | Open | Growing | Short/Mid/Long-term, Persona | No | No | No | Heat-based eviction | âœ… | No | âœ… | Standard | No |
| **Memoripy** | MIT-compat | ~1.5K | Short-term, Long-term | No | No | No | Memory decay model | âœ… | No | âœ… | Minimal | No |
| **Hindsight** | MIT | ~4K | Facts, Entities, Episodic | âœ… | âœ… | âœ… (4-strategy) | Entity resolution + reranking | âœ… | âœ… | âœ… | 2 GB | No |
| **GraphRAG** | MIT | ~24K | Institutional (docs) | âœ… | No | No | Static (offline rebuild) | âœ… | No | âœ… | High | Recommended |

*LlamaIndex framework total

***

## Benchmark Performance Summary

> **Important caveat for LLM consumers:** Benchmark claims in this domain are frequently disputed. Mem0's self-reported LOCOMO scores have been challenged by Zep's independent replication. Zep's own claims were also challenged by Mem0. Independent third-party evaluations should be weighted above vendor self-reports. Neither LoCoMo nor LongMemEval tests whether memory actually improves agent task completion â€” they test conversational retrieval only.[^14][^30][^13][^7]

| Framework | LongMemEval | LoCoMo | DMR | Notes |
|---|---|---|---|---|
| **Hindsight** | **91.4%**[^7][^53] | â€” | â€” | Highest published; multi-strategy + reranking |
| **Graphiti/Zep** | 71.2% (GPT-4o)[^7] | 75.14% Â± 0.17[^13] | **94.8%**[^22] | Corrected from disputed 65.99%[^14] |
| **Letta (MemGPT)** | Not published[^8] | ~74â€“83%[^21][^12] | 93.4%[^22] | DMR benchmark originated from MemGPT team |
| **Mem0** | 49.0% (independent)[^8] | ~66â€“68.5%[^12] | â€” | Self-reported LOCOMO disputed[^30][^14] |
| **MemoryOS** | â€” | +49% F1 vs. baseline[^46] | â€” | EMNLP 2025 Oral; relative improvement, not absolute |
| **LangMem** | Not published | Not published | â€” | â€” |
| **LlamaIndex Memory** | Not published | Not published | â€” | â€” |
| **Cognee** | Not published | Not published | â€” | 200K+ production pipeline runs reported[^2] |
| **Memoripy** | Not published | Not published | â€” | â€” |
| **GraphRAG** | Not applicable | Not applicable | â€” | Different use case target |

***

## Scalability Analysis

| Framework | Multi-Tenant | Horizontal Scaling | Max Storage | Production-Validated | Enterprise Support |
|---|---|---|---|---|---|
| **Mem0** | âœ… (scoped by user/session/agent) | âœ… (vector DB sharding) | Managed cloud | âœ… ($24M funded, many deployments) | âœ… SOC2/HIPAA |
| **Letta** | âœ… (per-agent isolation) | âœ… (Aurora up to 256 TB[^16]) | 256 TB (Aurora) | âœ… | âœ… |
| **Graphiti** | âœ… (per-user graphs, concurrent) | âœ… (Neo4j clustering) | Graph-native | âœ… (Zep platform) | Via Zep Cloud |
| **Cognee** | âœ… (user/tenant isolation) | âœ… (PG production mode[^34]) | PG-bounded | Partial (newer cloud) | On-prem â‚¬1,970/mo |
| **LangMem** | Via LangGraph namespaces | âœ… (Postgres backend) | PG-bounded | Partial | No |
| **LlamaIndex Memory** | Via LlamaCloud | âœ… (LlamaCloud) | Cloud-bounded | Via LlamaCloud | Via LlamaCloud |
| **MemoryOS** | Limited | No | Local storage | No (academic) | No |
| **Memoripy** | No | No | Local file | No | No |
| **Hindsight** | âœ… | âœ… (Postgres) | PG-bounded | Growing | Via cloud |
| **GraphRAG** | Via Azure | âœ… (Azure AI) | Azure-bounded | âœ… (Microsoft) | âœ… Azure |

***

## Long-Term Performance: Anti-Degradation Mechanisms

| Framework | Semantic Drift Mitigation | Noise Accumulation Control | Temporal Consistency | Memory Compaction | Agent Drift Mitigation |
|---|---|---|---|---|---|
| **Mem0** | LLM-delegated extraction (partial) | LLM-decides importance (partial) | No explicit versioning | Consolidation at 10K memories | Weak (LLM-dependent) |
| **Letta** | Agent self-editing (non-deterministic) | Agent eviction | Recall/archival separation | Agent-driven | Model-dependent |
| **Graphiti** | **Temporal versioning (validity windows)** | Append-only episodic log | **Best-in-class** | Async background | Strong (temporal rollback) |
| **Cognee** | **memify() RL optimization** | Hierarchical abstraction layers | Graph edge timestamps | Auto-tune pruning | Adaptive weight strengthening |
| **LangMem** | Background manager (basic) | Explicit pruning recommended[^42] | None | Manual compression[^42] | Weak |
| **LlamaIndex Memory** | Summary buffer only | FIFO flush | None | Summary on overflow | Weak |
| **MemoryOS** | Persona module evolution | **Heat-based eviction** | Mid/long-term promotion | FIFO-to-summary pipeline | Persona adaptation |
| **Memoripy** | **Forgetting curve decay** | Decay + clustering | None | Semantic cluster consolidation | Moderate |
| **Hindsight** | Entity resolution (dedup) | Multi-strategy reranking filters noise | Temporal filtering | Write-time extraction | Cross-encoder reranking |
| **GraphRAG** | Static (not dynamic) | Offline rebuild only | No | Requires full re-index | Not applicable |

***

## Hardware and Infrastructure Requirements

| Framework | Minimum RAM | Storage | Graph DB Required | GPU | Docker | Local-Only Capable |
|---|---|---|---|---|---|---|
| **Mem0** | 4 GB[^15] | External vector DB | No (Pro only) | No | Yes | Yes |
| **Letta** | 512 MB (1 GB+ prod)[^23] | LanceDB (default) or PG | No | No | Yes | Yes |
| **Graphiti** | 4 GB+ (Neo4j)[^55] | Graph DB storage | **Yes** | No | Yes | Yes |
| **Cognee** | ~512 MB[^34] | SQLite/LanceDB/Kuzu (embedded) | Kuzu (embedded, no server) | No | Yes | **Yes (best)** |
| **LangMem** | ~256 MB | Postgres or in-memory | No | No | Via LangGraph | Yes |
| **LlamaIndex Memory** | ~256 MB | SQLite default | No | No | Yes | Yes |
| **MemoryOS** | ~512 MB | Local files + LLM | No | No | Yes | Yes |
| **Memoripy** | ~256 MB | Local JSON | No | No | Yes | Yes |
| **Hindsight** | 2 GB[^54] | Embedded Postgres | Embedded | No | Yes (1 cmd) | Yes |
| **GraphRAG** | 8 GB+ | Large KG storage | Via Azure/Neo4j | Recommended | Via Azure | Partial |

***

## Top 3 Choices with Rationale

### ðŸ¥‡ Rank 1: Graphiti (by Zep)

**Recommended for:** Production multi-session agents where entities change over time, enterprise-grade temporal reasoning, compliance-critical applications.

**Rationale:**

Graphiti stands out as the most architecturally sound solution for the core problem of long-term memory: **things change over time**. No other framework explicitly models when facts become true and when they are superseded, using validity-windowed temporal edges indexed for efficient historical queries. This directly prevents the most common form of agent failure in production: acting on stale information.[^26][^7]

The performance profile is best-in-class where it matters for production: sub-300ms retrieval at P95 (Zep Cloud) because all computation â€” entity extraction, relationship resolution, embedding generation, and graph construction â€” happens at write time. Queries never trigger LLM calls. The DMR benchmark (94.8%) and corrected LoCoMo score (75.14%) are among the highest of any framework.[^28][^1][^13][^22]

For long-term stability, Graphiti is the only framework with an explicit architecture for temporal rollback: validity windows combined with an append-only episodic log mean that if a fact changes, the old fact is not deleted but marked as having ended â€” enabling historical reasoning and preventing the semantic drift that degrades other systems over time. The 50% token reduction from entropy-gated extraction (Nov 2025 release) also reduces the primary operational cost of graph construction.[^27][^3]

Community momentum is compelling: 24K+ GitHub stars, 20K milestone hit in November 2025 from a base of ~2K in early 2025 (650%+ growth in under 6 months), hundreds of thousands of weekly MCP server users, and enterprise contributors including AWS, Microsoft, Neo4j, and FalkorDB.[^56][^26][^27]

**Trade-off to accept:** Ingestion latency for large corpora is high (can take hours for very large datasets); requires a graph database (Neo4j, FalkorDB, or Kuzu); Zep Community Edition is deprecated so full feature self-hosting now means working directly with the Graphiti library rather than a packaged service.[^1][^7]

***

### ðŸ¥ˆ Rank 2: Mem0

**Recommended for:** Teams needing the broadest ecosystem compatibility, fastest time-to-production, conversational personalization at scale, or multi-framework agent deployments.

**Rationale:**

Mem0's 48K GitHub stars and $24M Series A funding are not vanity metrics â€” they reflect genuine production adoption at scale. Mem0 is the only framework in this comparison with a native TypeScript SDK, Python SDK, framework integrations across LangChain, CrewAI, LlamaIndex, and more, SOC 2 and HIPAA compliance on the managed platform, and a managed-to-self-hosted pathway with no vendor lock-in (Apache 2.0). When evaluating any dependency for production use, the ecosystem around it â€” the available integrations, the documentation quality, the community size for debugging support â€” matters as much as raw benchmark numbers.[^8][^7]

On the technical side, Mem0's architecture is pragmatic and production-tested: atomic fact extraction delegates decisions to the LLM, which is non-ideal for deterministic systems but results in flexible, context-aware memory management. Memory consolidation handles 10K+ memories per user with sub-100ms update latency. The 91% p95 latency reduction and 90% token savings vs. full-context approaches are independently reproducible from the arxiv paper. The key weakness â€” LongMemEval scores significantly lower in independent evaluations (49.0%) â€” reflects architectural simplicity, not a fundamental flaw. For most production applications that do not require complex temporal or multi-hop relational reasoning, Mem0's architecture is sufficient and its operational characteristics are well understood.[^57][^11][^17][^8]

The OpenMemory MCP server enables fully local deployments with no cloud dependency, addressing privacy-sensitive use cases.[^18]

**Trade-off to accept:** Graph features require the $249/month Pro tier on the managed platform (self-hosted does not gate graph features, but the open-source graph implementation differs from managed Pro). Independent benchmark evaluations score significantly lower than self-reported numbers. For applications requiring deep temporal reasoning or multi-hop relational queries, Graphiti is a better fit.

***

### ðŸ¥‰ Rank 3: Cognee

**Recommended for:** Local-first or air-gapped deployments, multimodal data ingestion, cost-sensitive production environments, teams prioritizing privacy and full infrastructure ownership.

**Rationale:**

Cognee occupies a unique position: the only framework in this comparison that is simultaneously fully local (no cloud dependency, no GPU), multimodal (text, PDFs, images, audio, code), knowledge-graph-capable at all tiers (no paywall for graph features), and specifically architected to address long-term memory degradation through its `memify()` pipeline.[^33][^32][^35]

The default stack (SQLite + LanceDB + Kuzu) runs on any modern CPU with approximately 512 MB RAM, making it viable for embedded systems, Raspberry Pi clusters, edge deployments, and cost-constrained homeserver environments. The ECL pipeline (Extract, Cognify, Load) provides a structured approach to ingesting enterprise knowledge bases â€” 30+ connectors including document, image, and audio sources â€” without requiring custom integration code.[^32][^34]

The `memify()` pipeline is architecturally significant for long-term stability: it uses RL-inspired optimization to strengthen frequently useful memory pathways, prune obsolete nodes, and auto-tune based on real usage. This directly addresses the consolidation equilibrium problem that causes pure-vector systems to degrade over time as noise accumulates. The hierarchical memory abstraction (raw episodes â†’ session traces â†’ meta-nodes â†’ shared world model) provides principled multi-granularity storage that scales both informationally and computationally.[^35][^2]

With 14.7K GitHub stars, â‚¬7.5M seed funding, and graduation from GitHub's Secure Open Source Program, Cognee has established sufficient production credibility for enterprise evaluation.[^31][^38]

**Trade-off to accept:** Python-only (no TypeScript or Go SDKs); no published standardized benchmark scores (LongMemEval, LoCoMo); managed cloud offering is newer and less battle-tested than Mem0 or Zep. Teams requiring TypeScript support or cross-framework SDK flexibility should use Mem0 or Graphiti instead.[^7]

***

## Decision Guide for LLM Consumers

Use the following signal mapping to select the appropriate framework:

| Signal | Recommended Framework |
|---|---|
| Need entity/relationship history over time | Graphiti |
| Largest community + most integrations | Mem0 |
| Zero cloud dependency, fully local, multimodal | Cognee |
| Already using LangGraph heavily | LangMem |
| Already using LlamaIndex, need basic session memory | LlamaIndex Memory |
| Highest retrieval accuracy needed, OK with newer tooling | Hindsight |
| Personal assistant with human-like personalization | MemoryOS or Memoripy |
| Need agents to self-manage their own context | Letta |
| Large static document corpus (not conversational) | Microsoft GraphRAG |
| Privacy-first, minimal infrastructure, offline | Memoripy or Cognee |

***

## Known Gaps and Limitations of This Report

1. **No standardized task-completion benchmarks exist** as of Q1 2026. LoCoMo and LongMemEval measure conversational retrieval, not whether memory improves agent task performance over time. This is the most critical gap for production evaluation.[^7]
2. **Benchmark numbers are contested.** Vendor self-reports should be treated skeptically. Zep's LoCoMo score shifted from 65.99% to 75.14% when implementation errors were corrected. Mem0's LOCOMO claims have been disputed. Independent third-party evaluation is strongly recommended.[^14][^13]
3. **Long-term stability data is limited.** Most frameworks are young (2023â€“2025). Production reports of memory degradation at 100K+ memories or 1,000+ sessions are sparse in public literature.
4. **Hardware requirements are framework-layer only.** All frameworks require an external LLM for extraction and/or retrieval. The LLM's own hardware requirements (API costs or local inference hardware) are additive.
5. **Hindsight and MemoryOS are newer projects.** Despite strong benchmark performance, their production track records are shorter than Mem0, Letta, or Graphiti. Evaluate thoroughly before adopting for mission-critical applications.

---

## References

1. [Yohei (@yoheinakajima) on X](https://x.com/yoheinakajima/status/2037201711937577319) - The proliferation of memory benchmarks in 2024-2026 reflects both the field's importance and dissati...

2. [Anthony Alcaraz's Post - LinkedIn](https://www.linkedin.com/posts/anthony-alcaraz-b80763155_the-equilibrium-problem-in-agentic-memory-activity-7405183320281473024-5o0X) - The equilibrium problem in agentic memory is more nuanced than most engineers realize. âš–ï¸ I've been ...

3. [5 Why Memory Fails: Drift...](https://arxiv.org/html/2603.11768v1)

4. [Quantifying Behavioral Degradation in Multi-Agent LLM Systems Over ...](https://arxiv.org/html/2601.04170v1) - This study introduces the concept of agent driftâ€”the progressive degradation of agent ... Traditiona...

5. [Comparing Memory Systems for LLM Agents: Vector, Graph, and ...](https://www.marktechpost.com/2025/11/10/comparing-memory-systems-for-llm-agents-vector-graph-and-event-logs/) - 2.1 Temporal Knowledge Graph Memory (Zep / Graphiti) Â· 94.8% vs 93.4% accuracy over a MemGPT baselin...

6. [Memory OS of AI Agent](https://arxiv.org/html/2506.06326v1)

7. [Best AI Agent Memory Systems in 2026: 8 Frameworks Compared](https://vectorize.io/articles/best-ai-agent-memory-systems) - Your AI agent forgets everything between sessions. We ranked the 8 best agent memory systems in 2026...

8. [Mem0 vs Letta (MemGPT): AI Agent Memory Compared (2026)](https://vectorize.io/articles/mem0-vs-letta) - Mem0 vs Letta (MemGPT) â€” compare passive memory extraction with self-editing agent runtime for AI ag...

9. [Top 18 Open Source AI Agent Projects with the Most GitHub Stars](https://www.nocobase.com/en/blog/github-open-source-ai-agent-projects) - This article reviews the top 18 open-source AI Agent projects on GitHub by star count, analyzing the...

10. [I Benchmarked OpenAI Memory vs LangMem vs Letta (MemGPT) vs ...](https://www.reddit.com/r/LangChain/comments/1kash7b/i_benchmarked_openai_memory_vs_langmem_vs_letta/) - I verified its findings by comparing Mem0 against OpenAI's Memory, LangMem, and MemGPT on the LOCOMO...

11. [Mem0: Building Production-Ready AI Agents with Scalable Long ...](https://arxiv.org/abs/2504.19413) - We introduce Mem0, a scalable memory-centric architecture that addresses this issue by dynamically e...

12. [2026 AI Agent Memory Wars: Three Architectures, Three Philosophies](https://chauyan.dev/en/blog/ai-agent-memory-wars-three-schools-en) - AI Agent memory finally has serious solutions. Graph-based, OS-inspired, Observationalâ€”three archite...

13. [Is Mem0 Really SOTA in Agent Memory? - Zep](https://blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory/) - Mem0 recently published research claiming to be the State-of-the-Art in Agent Memory, besting Zep. I...

14. [Revisiting Zep's 84% LoCoMo Claim: Corrected Evaluation & 58.44 ...](https://github.com/getzep/zep-papers/issues/5) - our analysis shows that Zep achieves 58.44 % accuracyâ€”not the 84 % reported. This significant gap st...

15. [Pintaro/mem0 - GitHub](https://github.com/Pintaro/mem0) - System Requirements Â· Operating System: Windows 10 or newer, macOS, or Linux. Â· Memory: At least 4 G...

16. [How Letta builds production-ready AI agents with Amazon Aurora ...](https://aws.amazon.com/blogs/database/how-letta-builds-production-ready-ai-agents-with-amazon-aurora-postgresql/) - Storage scaling is also dynamic, growing from 10 GiB to 256 TB with no downtime, so your agents don'...

17. [Long-Term Memory for AI Agents: The What, Why and How - Mem0](https://mem0.ai/blog/long-term-memory-ai-agents) - Unlike token-limited buffers, long-term persistence survives resets, scales with storage, and is req...

18. [Introducing OpenMemory MCP - Mem0](https://mem0.ai/blog/introducing-openmemory-mcp) - AI memory OpenMemory MCP brings persistent memory to Claude Desktop. LLM memory integration for AI a...

19. [10 Open-Source LLM Frameworks Developers Can't Ignore in ...](https://zilliz.com/blog/10-open-source-llm-frameworks-developers-cannot-ignore-in-2025) - LLM frameworks simplify workflows, enhance performance, and integrate seamlessly with existing syste...

20. [The 6 Best AI Agent Memory Frameworks You Should Try in 2026](https://machinelearningmastery.com/the-6-best-ai-agent-memory-frameworks-you-should-try-in-2026/) - In this article, you will learn six practical frameworks you can use to give AI agents persistent me...

21. [5 AI Agent Memory Systems Compared: Mem0, Zep, Letta ...](https://dev.to/varun_pratapbhardwaj_b13/5-ai-agent-memory-systems-compared-mem0-zep-letta-supermemory-superlocalmemory-2026-benchmark-59p3) - A factual comparison of the five most-referenced AI agent memory systems on architecture, LoCoMo ben...

22. [A Temporal Knowledge Graph Architecture for Agent Memory - Zep](https://blog.getzep.com/zep-a-temporal-knowledge-graph-architecture-for-agent-memory/) - In this evaluation, Zep achieves substantial results with accuracy improvements of up to 18.5% while...

23. [Deploy Letta [Updated Mar '26] (Open-Source AI Agent Framework ...](https://railway.com/deploy/letta) - Deploying a managed Letta service on Railway gives you automation, scalability, and simplicity. Inst...

24. [Letta (MemGPT) Review: Key Features and Pros&Cons - XYZEO](https://xyzeo.com/product/letta-memgpt) - Letta (MemGPT) Production Requirements. Memory Storage Backend: Vector database (LanceDB default). L...

25. [Agent Memory](https://www.reddit.com/r/LocalLLaMA/comments/1gvhpjj/agent_memory/) - Agent Memory

26. [Graphiti Hits 20K Stars! + MCP Server 1.0 - Zep](https://blog.getzep.com/graphiti-hits-20k-stars-mcp-server-1-0/) - Graphiti crossed 20,000 GitHub stars today! Thanks for building with us. Graphiti is a temporal know...

27. [Daniel Chalef's Post - LinkedIn](https://www.linkedin.com/posts/danielchalef_graphiti-crossed-20000-github-stars-this-activity-7393723386985754625-uXTa) - Graphiti crossed 20,000 GitHub stars this week! ðŸŽ‰ Thanks to everyone building with us. What started ...

28. [Your LLM Has Amnesia: A Production Guide to Memory That ...](https://genmind.ch/posts/Your-LLM-Has-Amnesia-A-Production-Guide-to-Memory-That-Actually-Works/) - ... 2025, with GA targeted for Q1 2026 [8]. It ships with an ... Knowledge graphs beat flat memory f...

29. [Long Ingestation process Â· Issue #356 Â· getzep/graphiti - GitHub](https://github.com/getzep/graphiti/issues/356) - I have a question about dealing with very long ingestation processes when uploading data to the grap...

30. [Mem0: Building Production-Ready AI Agents with Scalable Long ...](https://arxiv.org/html/2504.19413v1)

31. [topoteretes/cognee](https://trendshift.io/repositories/13955) - Knowledge Engine for AI Agent Memory in 6 lines of code. Data last synced with GitHub about 7 hours ...

32. [Cognee - Open Source | Evermx](https://evermx.com/open-source/cognee-knowledge-engine-ai-agent-memory)

33. [Mem0 vs Cognee: AI Agent Memory Compared (2026) - Vectorize](https://vectorize.io/articles/mem0-vs-cognee) - Mem0 vs Cognee â€” compare the largest agent memory community with knowledge graph extraction for AI a...

34. [Deployment Guide | topoteretes/cognee | DeepWiki](https://deepwiki.com/topoteretes/cognee/9-deployment-guide) - This document provides an overview of Cognee's deployment architectures, infrastructure requirements...

35. [Building AI Agents with Persistent Memory with Cognee - LinkedIn](https://www.linkedin.com/posts/atul-anand-356319163_github-topoteretescognee-memory-for-ai-activity-7419977818928914433-hZ-_) - Drop your stack below #AI #RAG #GenerativeAI #LLMs #OpenSource ... Giving them long-term, reliable m...

36. [How Cognee Builds AI Memory Layers with LanceDB](https://lancedb.com/blog/case-study-cognee/) - How Cognee Builds AI Memory Layers with LanceDB ; Unified Storage: Original data and embeddings stor...

37. [Installation - Cognee Documentation](https://docs.cognee.ai/getting-started/installation) - Set up your environment and install Cognee

38. [cognee Graduates GitHub Secure Open Source](https://www.cognee.ai/blog/cognee-news/cognee-github-secure-open-source-program) - AI security meets open source AI memory as cognee graduates GitHub Secure Open Source, proving enter...

39. [Automated Knowledge Graphs with Cognee - Obsidian Forum](https://forum.obsidian.md/t/automated-knowledge-graphs-with-cognee/108834) - A year ago, I wrote about my Obsidian struggles after 2.5 years: hundreds of unconnected notes, lots...

40. [LangMem vs. Graphlit: LangChain Memory Framework vs. Full ...](https://www.graphlit.com/vs/langmem) - A comprehensive comparison of LangMem and Graphlit for agent memory. Understand when to use LangChai...

41. [Procedural Memory: Evolving...](https://blog.langchain.com/langmem-sdk-launch/) - Today we're releasing the LangMem SDK, a library that helps your agents learn and improve through lo...

42. [LangMem SDK for Agent Long-Term Memory](https://www.digitalocean.com/community/tutorials/langmem-sdk-agent-long-term-memory) - Explore the LangMem SDK for agent long-term memory features, architecture, and how it enables persis...

43. [LlamaIndex Memory: Building Smart Chat Agents - Kite Metric](https://kitemetric.com/blogs/llamaindex-memory-building-smart-chat-agents) - Enhance your AI chatbots with LlamaIndex memory. This guide shows how to implement short-term, stati...

44. [Improved Long & Short-Term Memory for LlamaIndex Agents](https://www.llamaindex.ai/blog/improved-long-and-short-term-memory-for-llamaindex-agents) - In this article, we will walk through some of the core features of the new LlamaIndex memory compone...

45. [Teaching the Llama to Remember | Hindsight](https://hindsight.vectorize.io/blog/2026/03/30/llamaindex-agent-memory) - LlamaIndex agents reset memory every session. Learn how to add persistent cross-session memory using...

46. [[2506.06326] Memory OS of AI Agent - arXiv](https://arxiv.org/abs/2506.06326) - Our pioneering MemoryOS enables hierarchical memory integration and dynamic updating. Extensive expe...

47. [[PDF] Memory OS of AI Agent - ACL Anthology](https://aclanthology.org/2025.emnlp-main.1318.pdf) - Grounded Memory (Ocker et al., 2025) integrates vision-language models for perception, knowl- edge g...

48. [Open-source â€œMemoryOSâ€ â€“ a memory OS for AI agents (Paper + Code)](https://www.reddit.com/r/AgentsOfAI/comments/1lujeu0/opensource_memoryos_a_memory_os_for_ai_agents/) - Open-source â€œMemoryOSâ€ â€“ a memory OS for AI agents (Paper + Code)

49. [General Agentic Memory Via Deep Research - arXiv](https://arxiv.org/html/2511.18423v1) - We evaluate GAM's performance through rigorous experimental studies. We jointly leverage the traditi...

50. [[EMNLP 2025 Oral] MemoryOS is designed to provide a memory ...](https://github.com/BAI-LAB/MemoryOS) - MemoryOS is designed to provide a memory operating system for personalized AI agents, enabling more ...

51. [Memoripy: AI Memory Made Smarter â€“ Now with OpenRouter ...](https://www.reddit.com/r/LocalLLaMA/comments/1h2941u/memoripy_ai_memory_made_smarter_now_with/) - Memoripy offers structured short-term and long-term memory storage to keep interactions meaningful o...

52. [MemoriPy - AI Tool For AI memory](https://theresanaiforthat.com/ai/memoripy/) - MemoriPy is an open-source AI memory layer designed to bring human-like memory and adaptability capa...

53. [Hindsight vs SuperMemory: Agent Memory Compared (2026)](https://vectorize.io/articles/hindsight-vs-supermemory) - Hindsight is MIT licensed. You can read the source, fork it, modify it, deploy it on your own infras...

54. [agentmemory.md â€” Persistent memory for your AI agent](https://agentmemory.md) - Give your AI agent a brain that persists across every session. Semantic search, knowledge graph, and...

55. [Graphiti messages are added very slowly Â· Issue #186 - GitHub](https://github.com/getzep/graphiti/issues/186) - In our implementation of Graphiti at Zep we use low-latency LLM providers and self-hosted models to ...

56. [ðŸš€ MILESTONE ALERT: Graphiti just hit 15K GitHub stars! | Daniel Chalef | 13 comments](https://www.linkedin.com/posts/danielchalef_milestone-alert-graphiti-just-hit-15k-activity-7354540412688060417-Nz85) - ðŸš€ MILESTONE ALERT: Graphiti just hit 15K GitHub stars! ðŸ¥³ In under 6 months, Zep AI (YC W24) knowledg...

57. [GitHub All-Stars #2: Mem0 - Creating memory for stateless AI minds](https://virtuslab.com/blog/ai/git-hub-all-stars-2/) - It leads to two critical issues: Performance and cost degradation: Latency and cost are directly pro...
