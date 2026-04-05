# blue-lagoon

Phase 1 establishes a runnable Rust workspace with a harness-owned migration
path, schema safety checks, a no-trigger idle boot path, and a synthetic
end-to-end worker flow.

## Phase 1 Commands

Start PostgreSQL:

```bash
docker compose up -d postgres
```

The default local host mapping is `localhost:55432`.

Apply reviewed migrations:

```bash
cargo run -p runtime -- migrate
```

Verify the harness can boot safely and return to idle:

```bash
cargo run -p runtime -- harness --once --idle
```

Run the Phase 1 synthetic smoke trigger:

```bash
cargo run -p runtime -- harness --once --synthetic-trigger smoke
```

Run the baseline verification suite:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
docker compose config
```

`cargo test --workspace` boots local PostgreSQL with `docker compose up -d
postgres` when `BLUE_LAGOON_DATABASE_URL` is unset. Repository-hosted CI sets
`BLUE_LAGOON_DATABASE_URL` explicitly and uses a disposable GitHub Actions
PostgreSQL service instead.

## Phase 1.1 CI Baseline

The baseline repository-hosted workflow is `.github/workflows/ci.yml`. This is
the starting point for repository CI, not a one-off Phase 1 workflow. Later
phases should extend this baseline or add adjacent stable gates without
replacing its role as the core workspace verification path.

- Workflow name: `CI`
- Required check name: `workspace-verification`
- Triggers: `pull_request` and pushes to `master`

The Phase 1.1 local-to-CI command mapping is:

- `cargo fmt --all --check` -> `cargo fmt --all --check`
- `cargo check --workspace` -> `cargo check --workspace`
- `cargo test --workspace` -> `cargo test --workspace`

The following recurring Phase 1 commands remain intentionally outside the Phase
1.1 automation scope and must still be run locally when relevant:

- `docker compose config`
- `cargo run -p runtime -- migrate`
- `cargo run -p runtime -- --help`
- `cargo run -p runtime -- harness --once --idle`
- `cargo run -p runtime -- harness --once --synthetic-trigger smoke`

Repository follow-up after the workflow lands:

- require the `workspace-verification` check on `master` branch protection or the
  repository's equivalent ruleset
- keep this workflow as the foundation and extend the CI surface in later
  phases with additional stable checks as new runtime capabilities land
- release, deployment, and publish automation remain deferred
