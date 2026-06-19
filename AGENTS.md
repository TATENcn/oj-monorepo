# AGENTS.md

## Workspace layout

```
root/
  services/judge-core/   # Rust workspace
    crates/
      shared/            # models, binary protocol, socket helpers
      agent/             # in-container: compiles & sandboxes user code
      manager/           # on-host: HTTP API, containerd, agent pool
  apps/
    api/                 # TypeScript backend
    web/                 # frontend
  packages/              # shared TypeScript packages
    judge-core-sdk/      # HTTP client for judge-core API
    models/              # shared TS types
    utils/               # shared TS utilities
```

- **Package manager**: `bun` (not npm).
- **Nx**: thin task runner; `nx build judge-core` delegates to `cargo build`.
- All `cargo` commands must run inside `services/judge-core/`.

## Commands

```bash
# Dependencies
bun install

# Rust (from services/judge-core/)
cargo build --release
cargo build --release --bin manager   # Manager must run with root privileges

# Agent container image
fish scripts/build-agent.fish   # docker build → `ctr image import`
```

## Running the manager

**Prerequisite**: containerd daemon running, reachable at `/run/containerd/containerd.sock`, with a `judge-core` namespace. The agent image (`docker.io/library/judge-core:latest`) must already be imported.

```bash
cargo build --release --bin manager
sudo ./target/release/manager    # → HTTP on 0.0.0.0:8000
```

All configuration is hardcoded in `crates/manager/src/main.rs`. No automated tests — verify manually.

## Architecture

- **Manager ↔ agent**: Unix domain sockets with length-prefixed binary framing.
- **Sandboxing**: seccomp + cgroups v2 per test case.
- **Execution**: `POST /task` → dispatch to least-loaded agent → compile → run test cases in parallel → return verdict.
- **Auto-scaling**: pool scales between min/max agents based on queue load.

## Conventions

- **rustfmt**: `max_width = 160`, Rust edition 2024

## Key files

| Concern | Path |
|---|---|
| Manager entrypoint + config | `crates/manager/src/main.rs` |
| HTTP API | `crates/manager/src/router.rs` |
| Agent pool + dispatch | `crates/manager/src/pool.rs` |
| Container CRUD | `crates/manager/src/provisioner.rs` |
| Auto-scaling | `crates/manager/src/scaler.rs` |
| Binary protocol | `crates/shared/src/protocol.rs` |
| Verdict orchestration | `crates/agent/src/verdict/mod.rs` |
| C++ compile + execute | `crates/agent/src/verdict/cpp.rs` |
| seccomp whitelist | `crates/agent/src/limit/seccomp.rs` |
| cgroups v2 | `crates/agent/src/limit/cgroup.rs` |
| Agent Dockerfile | `crates/agent/Dockerfile` |
