<!-- source-hash: ba016ffcb1ed -->
# Agent Stream (`com.tasty.agent-stream`)

- **Status**: Implemented (the collection pipeline) — external emission (SSE) and inbound-webhook wiring are not implemented
- **Actors**: AI agent (CLI/IPC). No local-user UI — headless
- **Distribution / integration**: workspace bundle (registered in `BUILTINS`) · CLI + IPC namespace — [plugin concepts](../../concepts/plugins.md)
  - `bundle = false` — excluded from distribution packaging (DMG / AppImage / MSIX / deb). The workspace build's dev bundle sync still works as usual.
- **Code**: `crates/tasty-plugin-agent-stream/`
- **Permissions**: `surface.read` (reading the session-id meta · checking the target is alive) · `fs.read` (reading the transcript) · `fs.write` (writing the watch snapshot in data_dir)
- **Screens**: none
- **Rationale**: [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md)

> **As an example**: the **resident background thread + CLI/IPC namespace** example — the minimal shape for running a file-tail loop when the SDK has no async support → [plugin-development](../../dev-guide/plugin-development.md#cli--ipc-namespace) · [§10 limitations](../../dev-guide/plugin-development.md#10-한계-현재-sdk).

## Purpose

**Collects the responses of an AI coding agent running in a surface as structured events.** Unlike screen scraping (`tasty read screen` · `output-match` hooks), there is no ANSI, box-drawing, or line-wrapping noise mixed in, and thinking blocks are separated from response text right at the source.

The name is not Claude-specific because other agents such as codex differ only in transcript location — the tail, normalisation, and delivery are the same. **The only source interpreted today is Claude Code.**

## Internal behaviour

### Target resolution — surface_id → session id → transcript

1. The claude plugin's `SessionStart` hook records the session id in the surface meta `claude-session-id`.
2. This plugin reads that value with `surface.meta.get`. **If the meta is missing, the watch is refused** — accepting a registration when it cannot decide which file to watch would leave the caller believing a stream is attached while receiving nothing.
3. The session id is the file name. The transcript root (`$CLAUDE_CONFIG_DIR/projects`, or `~/.claude/projects` when unset) is **scanned one level down** for `<session-id>.jsonl` — the project-slug rule is never computed (ADR-0093).

There is **no code dependency** on the claude plugin. The only point of contact is one surface-meta key read over host IPC.

### The tail loop

Runs on a **dedicated thread** so as not to break the host healthcheck (15 s ping / forced restart after 60 s without a reply). It has two cadences.

| Cadence | Work |
|------|---------|
| 300 ms | Read the file from the offset and turn completed lines into events. No host IPC calls |
| 3 s (every 10 ticks) | Confirm the target is alive with `surface.locate`; check for a session switch with `surface.meta.get` |

Every abnormal file state is handled.

| State | Detection | Response |
|------|------|------|
| Not created yet (a race right after session start) | `metadata` is `NotFound` | Wait as `awaiting_transcript`, re-resolve the path every tick |
| Deleted while reading | same as above | Read from the start again once it is recreated |
| Truncated midway | `len < offset` | Resync from 0 |
| Rotated / file replaced | inode (Unix) · file index (Windows) changed | Resync from 0 |
| Trailing partial line without a newline | leftover in the buffer | Held until the next read completes it — emitted **exactly once** on completion |

If a resync re-reads the same record, deduplication by record `uuid` (the last 4096 remembered) absorbs it. Records without a `uuid` have no basis for claiming sameness and pass through as-is.

### Event model

Only the `message.content[]` blocks of `assistant` records and turn ends become events.

| kind | Source | Fields carried |
|------|------|-----------|
| `text` | `content[].type == "text"` | `text` |
| `thinking` | `content[].type == "thinking"` (the body key is `thinking`) | `text` |
| `tool_use` | `content[].type == "tool_use"` | `tool_name` · `tool_input` |
| `turn_end` | the table below | `reason` |

Every event carries `seq` (globally monotonic) · `surface_id` · `session_id`; events from the file also carry `record_uuid` · `timestamp`.

> **The body of `thinking` may be empty.** Depending on the Claude Code version/settings, the transcript's `thinking` block is recorded with only a `signature` and an empty body (observed). In that case the `thinking` event's `text` is an empty string — nothing absent from the source is invented; it is relayed as-is. The kind split is still valid, so consumers can still pick out and discard thinking blocks.

**Turn-end reasons** — handling only normal completion would leave consumers waiting forever, so every abnormal path is included.

`reason` **distinguishes its origin by prefix.** `stop:` carries the transcript's `stop_reason` verbatim; `stream:` is a reserved reason this pipeline decided on. Without the prefix, if the external spec ever started using a string like `session_ended` as a `stop_reason`, consumers could not tell the two apart. Consumers separate reserved reasons with `reason.starts_with("stream:")`.

| `reason` | When |
|----------|------|
| `stop:end_turn` / `stop:max_tokens` / … | `assistant.message.stop_reason` with `stop:` prefixed (but `tool_use` is not a turn end — the turn continues after the tool result) |
| `stream:api_error` | `isApiErrorMessage: true` — API error responses arrive with an ordinary `stop_reason`, so this is checked first |
| `stream:cancelled` | a `user` record with the `[Request interrupted by user…]` marker |
| `stream:session_ended` | the target surface disappeared, or a new session started on the same surface and closed the previous one |
| `stream:unwatched` | `agent_stream.unwatch` was called |
| `stream:rewatched` | the same surface was `watch`ed again, replacing the previous registration |

User prompt bodies · tool results · attachments · mode switches and every other record are **not relayed** (non-goals).

### Session switch

When the verify tick finds that the session id in the surface meta has changed, it leaves `turn_end{reason=stream:session_ended}` on the old session and switches the tail target to the new file. The new session file is read **from the beginning** (the whole of that session is in scope). If the new file does not exist yet, the path stays unresolved and is re-resolved every tick.

### Restart recovery — at-least-once

The watch targets and byte offsets are kept in `TASTY_PLUGIN_DATA_DIR/watches.json`, and on plugin restart the tail resumes **from the saved offset**. Writes go to a `.json.tmp` in the same directory followed by a rename — an atomic replace, so dying mid-save never leaves a half-written snapshot. Records after the last flush may be read again — **a decision to prefer duplicates over gaps** (ADR-0093). Consumers fold duplicates by `record_uuid`.

The `seq` cursor is kept in the same snapshot. If it restarted from 1 each time, a consumer holding `after_seq` would silently miss the first N events after a restart — duplicates are tolerated, silent gaps are not. However, **the buffer contents themselves live only in memory and are lost on restart** (only the meaning of the cursor is preserved).

In an abnormal launch where `TASTY_PLUGIN_DATA_DIR` was not injected, persistence is **skipped** (nothing is quietly written elsewhere).

## Interface

- **AI agent**: both the `tasty agent-stream …` CLI and the `agent_stream.*` IPC. No GUI-only path.
- **Local user**: none (headless).

### CLI / IPC

| CLI | IPC method | Description |
|-----|-----------|------|
| `tasty agent-stream watch [--surface N] [--from-start]` | `agent_stream.watch` | Start tailing. `TASTY_SURFACE_ID` when `--surface` is omitted. Default is from the current end of file — `--from-start` reads from the beginning |
| `tasty agent-stream unwatch [--surface N]` | `agent_stream.unwatch` | Stop tailing + `turn_end{reason=stream:unwatched}` |
| `tasty agent-stream list` | `agent_stream.list` | List every target (focus-independent). `status` is `tailing` / `awaiting_transcript` |
| `tasty agent-stream poll [--surface N] [--after-seq S] [--limit L]` | `agent_stream.poll` | **Non-destructive** read by seq cursor. Several consumers read the same buffer with their own cursors |

`watch` only supports **an explicitly given surface_id** — there is no "watch everything" wildcard (ADR-0093 decision 3). `watch`ing the same surface again replaces the previous registration (`replaced: true`) and leaves `turn_end{reason=stream:rewatched}` on it.

**The `--from-start` exception** — even when registered without `--from-start`, if the transcript file did not exist at registration time (`awaiting_transcript`), it is read **from the beginning** the moment it is found. There was no file to serve as the "current end", so the whole of that session is in scope. The same applies when a session switch changes the file.

The `--surface` of `poll` has the parameter name `filter_surface`. The CLI layer auto-fills a u32 argument named `surface` with `TASTY_SURFACE_ID`, so leaving it as `surface` would silently narrow the query to the caller's own surface even when not specified.

The collection buffer is a ring capped at 4096. On overflow the oldest are dropped and the `poll` response reports it in `dropped`.

## Non-goals

- **External emission (SSE)** — this plugin only collects. The emission channel is separate.
- **Inbound webhook wiring** — the path that lets an external request drive an agent is separate. The host listener in that direction is [webhook](../../features/webhook/index.md).
- **Relaying user prompts · tool results** — only what the agent produced becomes events.
- **codex transcripts** — the name allows for it, but the only source interpreted today is Claude Code.
- **Writing/modifying transcripts** — read-only.

## Acceptance Criteria

- [ ] Given a surface running an agent Then `agent-stream watch` resolves the session id and returns the transcript path.
- [ ] Given the agent responds in a watched surface Then within seconds that text appears in `poll` as a `text` event.
- [ ] Given a response with a thinking block Then `thinking` and `text` come out as different kinds.
- [ ] Given a surface with no `claude-session-id` meta Then `watch` is refused with a clear error (no silent no-op).
- [ ] Given a turn ends by error / cancellation / session end Then a `turn_end` with the matching `reason` is emitted.
- [ ] Given the plugin is `disable && enable`d Then tailing resumes from the saved offset (duplicates allowed).

## Related

- [ADR-0093](../../adr/0093-agent-response-relay-reads-transcript-jsonl.md) — rationale for the source, delivery guarantee, and targeting
- [claude](../claude/index.md) — the side that records the `claude-session-id` surface meta
- [dev-guide/plugin-development](../../dev-guide/plugin-development.md) — §9.1 deployment procedure · §10 limitations
- [features/terminal-output](../../features/terminal-output/index.md) — screen-based output structuring (a different source)
