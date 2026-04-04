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
