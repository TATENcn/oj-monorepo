# AGENTS.md

## Workspace layout

```
onlinejudge/
├── packages/
│   ├── algorithm/             # Rating & ranking algorithms
│   └── utils/
├── services/crates/
│   ├── algorithm/             # Rating & ranking algorithms
│   ├── api_server/            # HTTP API — Axum, SeaORM
│   ├── api_server_auth/       # Auth endpoints
│   ├── api_server_db/         # Shared database models & queries
│   ├── api_server_submission/ # Submission ingest → RabbitMQ
│   ├── auth/                  # Ed25519 JWT sign/verify + password hashing
│   ├── gateway/               # Reverse proxy with rate limiting & auth
│   ├── judge_core_agent/      # Sandboxed compile/run, Unix socket server
│   ├── judge_core_manager/    # Agent pool + autoscaler + HTTP router (port 8000)
│   ├── judge_core_sdk/        # Rust HTTP client for judge_core_manager
│   ├── judge_core_shared/     # Shared models, HTTP types, wire protocol, error codes
│   ├── judge_core_standalone/ # Single-binary agent, HTTP /task endpoint
│   ├── submission_processor/  # RabbitMQ consumer → judge_core bridge
│   └── service_utils/         # Shared service boilerplate (graceful shutdown)
├── scripts/build-agent.fish   # Build & load agent image into containerd
├── nx.json                    # Task orchestration, caching
├── biome.json                 # Formatter/linter
├── package.json               # Bun workspace root
└── tsconfig.json
```

Bun workspaces + Cargo workspace. Nx for task orchestration.

## Running services

**Judge core — manager + agent (production, requires root):**

```fish
fish scripts/build-agent.fish
cd services && sudo cargo run -p judge_core_manager
```

**Judge core — standalone (single binary, no containerd):**

```fish
cd services && cargo run -p judge_core_standalone
```

**Submission processor:**

```fish
cd services && RABBIT_MQ_URL=amqp://... JUDGE_CORE_URL=http://localhost:8000 cargo run -p submission_processor
```

## Architecture

### Submission pipeline

```
API (POST /submissions) → RabbitMQ (submit.queue)
  → submission_processor → POST /task to manager (port 8000)
  → manager dispatches to agent via Unix socket
  → agent sandbox-compiles and runs
  → result flows back through RabbitMQ (result.queue)
  → API consumer updates DB
```

### Manager ↔ Agent wire protocol

Binary postcard frames over Unix stream. Data frame: 4-byte LE length prefix → `Frame<T> { id: u64, inner: T }`. Heartbeat: `u32::LE(0)` echoed back.

### Agent sandboxing

containerd containers with user/PID/mount/network/cgroup namespaces, cgroup v2 limits, seccomp filtering, `/work` tmpfs. Requires root (containerd socket at `/run/containerd/containerd.sock`).

## Key files

| File | Role |
|---|---|
| `services/crates/api_server/src/main.rs` | API entry point |
| `services/crates/api_server_auth/src/main.rs` | Auth microservice (/token, /revoke, /introspect, /jwks) |
| `services/crates/api_server_submission/src/main.rs` | Submission ingest → RabbitMQ |
| `services/crates/auth/src/token.rs` | Ed25519-dalek JWT generation/verification |
| `services/crates/gateway/src/main.rs` | Reverse proxy + JWT auth + rate limiting |
| `services/crates/judge_core_shared/src/models/mod.rs` | `VerdictTask`, `VerdictTaskResult`, `Language`, verdict types |
| `services/crates/judge_core_shared/src/models/http.rs` | `VerdictResponse`, error codes (`ERR_*`), `PoolMetrics` |
| `services/crates/judge_core_shared/src/protocol.rs` | Wire protocol + heartbeat framing |
| `services/crates/judge_core_sdk/src/lib.rs` | Rust HTTP client for manager |
| `services/crates/judge_core_manager/src/main.rs` | Manager — containerd, pool, autoscaler, Axum router |
| `services/crates/judge_core_agent/src/main.rs` | Agent — Unix socket listener |
| `services/crates/judge_core_standalone/src/main.rs` | Standalone agent — HTTP /task endpoint |
| `services/crates/submission_processor/src/main.rs` | RabbitMQ consumer → judge_core bridge |
| `services/crates/service_utils/src/lib.rs` | Axum serve + graceful shutdown helper |

## Conventions

- **Error codes**: `judge_core_shared::models::http` (`ERR_QUEUE_FULL`, `ERR_TASK_TIMEOUT`, etc.)
- **Serialization**: serde internally-tagged enums (`tag = "status"`, `tag = "case_status"`)
- **Formatting**: Biome (TS), rustfmt (Rust)
- **Dependencies**: internal path crates + shared external deps (≥2 consumers) in `services/Cargo.toml` `[workspace.dependencies]`; single-use external deps stay in crate Cargo.toml
