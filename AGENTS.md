# Repository Guidelines

## Project Structure & Module Organization
`blue-lagoon` is currently a docs-first repository with a completed implementation-design baseline. Root documents such as `README.md` and `PHILOSOPHY.md` define project identity and decision principles. Authoritative design material lives in `docs/`, especially `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`. Use `docs/sources/` for research inputs and external references. Historical handover documents live in `docs/archive/` and should be treated as archived context rather than current canonical guidance.

## Build, Test, and Development Commands
There is no runnable application, build pipeline, or automated test suite in the repository yet. Typical contribution workflow is still document editing plus review:

- `rg --files` to inspect the repository quickly.
- `git diff -- docs/ PHILOSOPHY.md` to review prose changes before commit.
- `git log --oneline` to match existing commit style.
- `markdownlint "**/*.md"` if available locally, to catch heading and spacing issues.

When implementation begins, extend this guide with the real build, test, lint, and migration commands defined by `docs/IMPLEMENTATION_DESIGN.md`.

## Coding Style & Naming Conventions
Write in Markdown with clear ATX headings (`#`, `##`, `###`) and short paragraphs. Keep language precise, technical, and directive. In formal specs, preserve normative terms such as `MUST`, `SHOULD`, and `MAY`. Follow existing naming patterns: top-level canonical documents use uppercase names like `PHILOSOPHY.md`, while supporting material under `docs/sources/` and archived handovers under `docs/archive/` use descriptive lowercase kebab-case filenames.

## Testing Guidelines
Documentation testing is manual for now. Verify that section hierarchy is consistent, terminology matches `PHILOSOPHY.md`, `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, and `docs/IMPLEMENTATION_DESIGN.md`, and cross-document claims do not conflict. Re-read modified files in rendered Markdown when possible. Treat broken links, contradictory definitions, and unclear scope boundaries as defects that must be fixed before merge.

## Commit & Pull Request Guidelines
Existing history uses short, direct subjects such as `initial docs`. Keep commit messages brief and imperative, and avoid bundling unrelated document changes together. Pull requests should state which documents changed, why the change is needed, and any open questions or follow-up work. Include screenshots only when a rendered artifact or visual document output materially changes.

## Source Material Handling
Do not treat `docs/sources/` as canonical product behavior; it is evidence and research context. Do not treat `docs/archive/` as current product guidance; it is retained for historical traceability. Promote conclusions into `docs/REQUIREMENTS.md`, `docs/LOOP_ARCHITECTURE.md`, `docs/IMPLEMENTATION_DESIGN.md`, or other canonical docs only after they are cleaned up, reconciled, and stated as repository-approved guidance.
