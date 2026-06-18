# tether-recall

Fleet bus proxy for recall memory queries — makes the laptop's recall store legible from the work node.

## Overview

The `recall` memory store is a local SQLite database on the laptop. When jsy is at the work node, wintermute is amnesiac about everything it has learned. `tether-recall` is a read-bridge: a session on the work node issues a `recall query` over the fleet bus (NATS/agorabus), the laptop runs it against the local store via the `recall` CLI, and ranked hits come back over the wire.

Read-first, no write-conflict risk. The self's thoughts become legible from the work side.

## Acceptance Criteria

1. **AC1 (MUST)**: Given a fixture recall, a `wm.fleet.recall.query` request for a term present in the fixture yields a result with expected hit(s) having `id`, `kind`, `subject`, `score`, and a bounded `snippet`.
2. **AC2 (MUST)**: Results are size-bounded: snippets truncated to configured max length, hit count never exceeds cap; `truncated:true` when clamping occurred.
3. **AC3 (MUST)**: `wm-tether-recall query` prints ranked hits in a format matching `recall query`'s columns.
4. **AC4 (MUST)**: The responder refuses mutating ops: write/forget verbs are rejected with an error reply; no write path is wired.
5. **AC5 (MUST)**: Requester timeout: with no responder present, exits non-zero within the bounded timeout and does not hang.
6. **AC6 (SHOULD, deferred)**: End-to-end round-trip selftest: requester → bus → responder → result, exactly one reply per request.
7. **AC7 (MUST)**: `cargo test` green; `sigpipe::reset()` first in `main()`; subprocess invocation uses no shell metacharacters.

## Install

```sh
cargo install --path . --bin wm-tether-recall
```

Or from the wintermute bootstrap:

```sh
~/.local/wintermute/bootstrap/install.sh
```

## Usage

```sh
# On the work node: query the laptop's recall store
wm-tether-recall query "wintermute memory bridge"

# With options
wm-tether-recall query "daily review" --kind reflective --limit 5

# Check responder reachability
wm-tether-recall status

# On the laptop: run the responder daemon
wm-tether-recall serve --nats-url nats://localhost:4222
```

## Environment

| Variable | Default | Description |
|---|---|---|
| `WM_NATS_URL` | `nats://localhost:4222` | NATS server URL |
| `WM_RECALL_BIN` | (PATH search) | Path to the `recall` binary |
| `RUST_LOG` | `warn` | Log level (tracing) |

## Architecture

- **Responder** (laptop): subscribes to `wm.fleet.recall.query`, invokes `recall query <text> --format json` as a subprocess (no shell), publishes results on `wm.fleet.recall.result.<req_id>`.
- **Requester** (work node): publishes a request, waits (bounded timeout) for the matching result, prints ranked hits.
- **Protocol**: JSON over NATS subjects. `QueryRequest` and `QueryResponse` types in `src/protocol.rs`.

## License

MIT OR Apache-2.0
