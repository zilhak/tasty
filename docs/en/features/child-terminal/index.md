<!-- source-hash: ddc397bc4268 -->
# child-terminal (host-native child terminal management · `tasty terminal`)

- **Status**: Implemented
- **Actor**: AI agent
- **ADR**: [ADR-0040](../../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md) (consumer of soft occupancy)
- **Code**: `src/core/child_terminal.rs` (registry) · `src/adapters/ipc/handler/terminal.rs` (IPC) · `crates/tasty-cli/src/commands/terminal.rs` (CLI)
- **Screens**: none — headless only (children render as ordinary terminal surfaces; the soft-occupancy border is the [surface-highlight](../surface-highlight/index.md)/occupancy contract)

## Purpose

Provides, as a host first-class feature, the **general machinery** with which an agent spawns / tells / kills child terminal surfaces. The same machinery used to be duplicated in the codex and claude plugins; the parts not tied to a specific agent binary (child registry, spawn composition, self-heal) are pulled up into the host and converged into a single source of truth (CLAUDE.md principle 2: agent features are IPC+CLI two-sided and host first-class).

## Internals (headless-valid)

- **registry** (`ChildTerminalRegistry`): holds `parent_surface → child list`, `child_surface → parent`, the next index per parent, the idle/needs_input state per child, and the time that state **was last reported** (`last_state_report_at`, unix epoch ms). Where the first two bool maps are "what was reported", the last value is "when it was reported" — a separate axis, refreshed on every hook push (`terminal.set_state`) and seeded by `register_child` with the registration time. It is the **hook-silence axis** of the derived state judgement (see "State judgement" below). Being epoch-based it survives host restarts. Persisted to `~/.tasty/child-terminals.json` (saved immediately on every register/remove; files persisted before this field existed become an empty map through `serde(default)`). It is a **different subsystem** from session token tracking (`session.rs`) and agent shell subprocess tracking (`runner_host` `shell_children`).
- **needs_input is shown on screen**: `terminal.set_state --state needs_input` (the agent hook
  entry point) itself only updates this registry and has no effect on the screen — the registry
  is the agent's completion-judgement input, not user UI state (inviolable principle 1). On-screen
  display is a separate channel, `AttentionKind::NeedsInput` of
  [surface-highlight](../surface-highlight/index.md); the Claude plugin calls both
  `terminal.set_state` and `surface.completion { kind: "needs_input" }` **from the same hook
  event** so both sources of truth are updated together
  (`crates/tasty-plugin-claude/src/hook.rs::apply_hook`). Only the `AttentionStore` record is
  cleared by focus — the user merely looking at the tab does not change the result of
  `tasty terminal state` (a query of this registry).
- **spawn**: creates a `terminal` tab in the target workspace's pane (sending the caller-specified `--command` verbatim), registers the child in the registry, then marks the child as **soft-occupied** (occupant = the parent surface that triggered the spawn). The command is an arbitrary string — agent-specific command builders stay in the plugins. **`command` is optional**: omit it and only the tab creation, registry registration, soft occupancy and the `child_surface_id` return happen, with nothing sent — the codex/claude plugins consume this **two-phase spawn** (first call without a command to register in the host registry, then send the agent-specific command with the returned surface_id baked in via `surface.send` separately; needed because of the surface_id inline env, session token, etc.).
- **soft-occupancy link**: on successful spawn `occupy_soft(child, parent)`, on kill `release_occupancy(child)`, on release `release_soft_occupancy(child, parent)` are called as **in-process core functions** (there is no `occupancy.*` IPC method — the ADR-0040 boundary). Soft occupancy only displays and does not block input (`attached=false`), and is cleared by the deferred focus cleanup when the parent dies.
- **self-heal**: since the host owns the live surface tree directly, at every access it reconciles against the live set and removes dead children from the registry (synchronously, no event subscription). The first access after boot reclaims leftovers from the previous session.
- **adopt**: unlike `terminal.spawn` (automatic only when creating a new tab), explicitly registers, right now, an arbitrary already-existing surface (whether it used to be a child or not) — it runs the same relationship-registration + occupancy sequence as `handle_spawn` without creating a PTY. Validation order: target exists → self-adoption refused (`parent == target`) → duplicate registration refused (already another parent's child) → hard occupancy (remote attach) refused → attempt `occupy_soft`. If `occupy_soft` fails (another parent already soft-occupies it), the registry is left untouched and an error is returned immediately (the `register_child`+`occupy_soft` order is the reverse of spawn — preserving the equivalence "children list = occupancy list").
- **release**: the counterpart of adopt — releases only the child relationship and the soft occupancy, without closing the surface (tab). It performs the same `remove_child`+`save` as `handle_kill`, but for the occupancy release it uses the occupant-checked `release_soft_occupancy(child, parent)` instead of the tier-agnostic `release_occupancy` — hard occupancy (remote attach) never consults `self.soft`, so structurally it is not touched. If `release_soft_occupancy` fails due to desync (e.g. the occupancy was already cleared first), only a `tracing::warn!` is left and the registry relationship removal still succeeds. The absence of a `surface.close` call is the only difference from kill.
- **Plugin signals that consume relationship existence**: the claude plugin's PTY error scanner decides whether a child surface is tracked by a `terminal.parent` query (relationship existence) — if it decided by surface existence (`surface.locate`), the **release** above leaves the surface behind and it would never be cleaned up. In other words `terminal.release` is also the boundary that cuts off `claude-error` firing for that child ([claude plugin](../../plugins/claude/index.md) "PTY error scan scope").

## State judgement (hook + observation fusion — [ADR-0072](../../adr/0072-child-state-hook-observation-fusion.md))

The `state` reported by `terminal.children` / `terminal.state` is **not** the registry's hook-push
cache read back verbatim. It is a derived value fusing the hook axis and the host observation axis,
and both IPC paths share the same helper (`CoreState::child_liveness{,_with_live}` —
`src/core/state/child_liveness.rs`).

**Why it is needed**: the registry's `state_of` returns `"active"` when both `idle`/`needs_input`
bools are false, but that means **"no evidence of idle"**, not "working". Since the only path that
changes the state is the one-way hook push, if a hook is lost or the child hangs, the last stamped
`active` stays forever and there is no path back.

### Judgement priority

Top to bottom, the first matching rule wins.

| # | Condition | `state` | `confidence` | `evidence` |
|---|---|---|---|---|
| 1 | surface absent from the live tree | `exited` | `confirmed` | `surface_gone` |
| 2 | hook reported `needs_input` | `needs_input` | `reported` | `hook_needs_input` |
| 3 | hook reported `idle` | `idle` | `reported` | `hook_idle` |
| 4 | PTY busy | `active` | `confirmed` | `pty_busy` |
| 5 | PTY not started (deferred) | `active` | `unobserved` | `pty_not_started` |
| 6 | foreground program returned to the shell | `stale` | `confirmed` | `foreground_is_shell` |
| 7 | output-silence duration unobservable (mirror, etc.) | `active` | `unobserved` | `observation_unavailable` |
| 8 | output silence < threshold | `active` | `heuristic` | `recent_output` |
| 9 | output silence ≥ threshold && hook silence < threshold | `active` | `heuristic` | `recent_hook_report` |
| 10 | output silence ≥ threshold && hook silence ≥ threshold | `stale` | `heuristic` | `output_and_hook_silent` |

- **2·3 ranking above observation** is deliberate — hooks never produce a false `idle`, so
  observation does not override these two.
- **5 ranking above 6–10** is deliberate too — a deferred terminal has never produced output at
  all, and without this gate every child would be misjudged `stale` right after spawn.
- Thresholds: output silence `CHILD_OUTPUT_SILENCE` = 120 s, hook silence `CHILD_HOOK_SILENCE` = 300 s.
  `BUSY_OUTPUT_WINDOW` (2 s) cannot be reused — that window means "is the screen updating right
  now" and is exceeded by the few seconds a person spends reading a prompt.
- Without a hook-silence reference point (entries persisted before this feature) it counts as
  silence — the output axis has already crossed its threshold, so neither axis contradicts it.

### Unregistered surfaces

`terminal.state` does not refuse queries for surfaces absent from the registry (the unregistered
fallback contract of `state_of` is kept). The response, though, is a derived judgement rather than
the raw registry value, so an arbitrary live surface **whose PTY is up and sitting at the shell
prompt** comes out as `stale` (`foreground_is_shell`) rather than `active` — exactly the observed
fact "no program is running in this surface". A surface whose PTY has not started (deferred) is
caught by the gate and stays `active` (`pty_not_started`).

### What `stale` means and its limits

`stale` is **not `exited`.** The surface is alive; it means "no agent process is running in this
surface, or there is no evidence that one is". A child brought in through `terminal.adopt` may be a
plain shell rather than an agent in the first place, so it must not be assumed terminated.

Output-silence-based stall detection is **a heuristic in principle** — a process stopped by SIGSTOP,
an agent in a long reasoning phase and a long command producing no output are observationally
indistinguishable. The only observations that can be treated as definitive are **surface absence**
and **foreground return to the shell**; everything else is marked `confidence: heuristic`. Consumers
can look at confidence and treat only definitive judgements as termination.

### Output only

`stale`/`exited` are values the host produces only through observation. `terminal.set_state` still
accepts only the three values `idle`/`needs_input`/`active` — if a hook could push derived states
into the registry, the observation axis would degrade back into a push cache.

### No active probing

Active probing, injecting input into the target surface to watch the reaction, is user-input
replay and therefore banned from release ([`docs/identity.md`](../../identity.md) principle 1), and
it also pollutes the child agent's prompt state. The judgement uses **passive observation only**.

### Cost

No additional process snapshot. The foreground program name is read only from the
`foreground_names` cache that the 1 Hz batch poll already fills — calling
`Terminal::foreground_process_info()` per child would be a regression reviving
O(surfaces × processes) (see the polling comment in `src/core/state/busy.rs`).

## Interface

- **AI agent (IPC/CLI)** — every target is named directly by ID (focus-independent, principle 3):
  - `tasty terminal spawn --workspace <ws> --command "<cmd>" [--surface <parent>] [--pane] [--cwd] [--role] [--nickname]` ↔ `terminal.spawn`
  - `tasty terminal tell "<text>" [--surface]` ↔ `terminal.tell`
  - `tasty terminal children [--surface]` ↔ `terminal.children`
  - `tasty terminal parent --surface <child>` ↔ `terminal.parent`
  - `tasty terminal state --surface <child>` ↔ `terminal.state` — single-child state query (`idle`/`needs_input`/`active`/`stale`/`exited`). Uses the **same judgement helper** (`CoreState::child_liveness`) as the per-item `state` of `terminal.children`, so list and single answers never diverge. A surface already cleaned out of the registry (gone via reconcile) is also checked directly against the live tree and identified as `"exited"` — the `"active"` fallback contract of `ChildTerminalRegistry::state_of` for unregistered surfaces is left intact, and the judgement layer above filters out dead surfaces on top of it
  - `tasty terminal kill [--surface] --child <n>` ↔ `terminal.kill`
  - `tasty terminal respawn [--surface] --child <n> [--cwd] [--command] [--role] [--nickname]` ↔ `terminal.respawn`
  - `tasty terminal broadcast "<text>" [--surface] [--role]` ↔ `terminal.broadcast`
  - `tasty terminal set-state --surface <child> --state <idle|needs_input|active>` ↔ `terminal.set_state` (agent hook entry point). **Derived states (`stale`/`exited`) are not accepted as input** — they are output only (see "State judgement" above)
  - `tasty terminal adopt --target <surface> [--surface <parent>] [--cwd] [--role] [--nickname]` ↔ `terminal.adopt` — without creating a new tab, explicitly registers an arbitrary already-existing surface as a child right now (soft occupancy)
  - `tasty terminal release [--surface <parent>] --child <n>` ↔ `terminal.release` — releases only the child relationship and the soft occupancy. The surface (tab) itself is not closed (unlike `terminal.kill`)

### Judgement response fields

Each item of `terminal.children` and the single `terminal.state` response carry **all** three
judgement axes. Both paths go through the same serialisation point (`liveness_fields`,
`src/adapters/ipc/handler/terminal.rs`), so the key set and values match structurally.

| Field | Values |
|---|---|
| `state` | `exited` \| `needs_input` \| `idle` \| `active` \| `stale` |
| `evidence` | `surface_gone` \| `hook_needs_input` \| `hook_idle` \| `pty_busy` \| `pty_not_started` \| `foreground_is_shell` \| `observation_unavailable` \| `recent_output` \| `recent_hook_report` \| `output_and_hook_silent` |
| `confidence` | `confirmed` \| `reported` \| `heuristic` \| `unobserved` |

The combinations that occur are exactly the rows of the "Judgement priority" table above — no
arbitrary combinations arise.

Looking at `state` alone **cannot distinguish differing grounds for the same value.** For
example `active` may mean "the PTY is producing output right now" (`pty_busy`/`confirmed`) or
"left as-is because there was no observation axis to judge by"
(`observation_unavailable`/`unobserved`). Consumers should read `confidence` and **treat only
definitive judgements as termination** — a `heuristic` `stale` is indistinguishable from SIGSTOP,
long reasoning and silent commands (see "What `stale` means and its limits" above), so terminating
a child on that alone kills a working child.

Raw observations (`busy`, output-silence duration, etc.) are not included — `evidence` already
tells "which axis decided the judgement", which serves the purpose, and freezing raw values into
the contract would turn threshold tuning into a consumer contract change.

**Consumer alignment**: the claude plugin's `claude.children` remap is a whitelist and copies the
three fields explicitly (`crates/tasty-plugin-claude/src/handlers.rs`). The codex plugin's
`codex.children` is passthrough and picks them up automatically.

### `--child <n>` is an index, not a surface_id

`--child` takes the **child index** issued from 0 per parent (`ChildTerminalRegistry::next_index_for`).
It is a different number space from the `surface_id` that `terminal.children` returns, and since both
are integers they are easy to confuse. The two spaces **can genuinely overlap** — a fresh instance
numbers surface ids 1, 2, 3… too, so `--child 2` is structurally ambiguous between index 2 and surface
2. Hence the passed value is not auto-interpreted as a surface_id; **the argument's meaning stays fixed
as index and the error message guides you**:

| Value passed | Response |
|---|---|
| a `child_surface_id` of the same parent | `… 4 is a child_surface_id, not a child index — use \`--child 2\`` |
| a `child_surface_id` of a different parent | `… under a different parent — use \`--surface 9000 --child 4\`` |
| anything else (typo, out of range, already cleaned up) | `… (valid child indices: 0, 2; 2 children)` |

kill/release/respawn all use the same messages. Failure is `exit=1` + stderr, so a batch script
**must check the exit code per call** — discarding it mistakes total failure for success.

### Omitting `--surface` with multiple windows

`kill`/`release`/`respawn`/`broadcast` may omit `--surface` (the parent) — the host falls back to the
current engine's `child_terminals.single_parent()` (succeeds only when there is exactly one parent;
0 or 2+ is an error). This fallback checks uniqueness **only within that engine (= one main
window)** — in a session with 2 or more main windows open, which window to look at is undefined in
the first place. So when these 4 methods are called without `--surface` (and without any other
routable resource id) while 2 or more main windows exist, instead of silently leaking to the
focused window the routing stage (`App::find_request_owner`) rejects with an explicit error
(single-window sessions may still omit it — backward compatible). Implementation:
`src/app/request_owner.rs` `ambiguous_parent_fallback_requires_surface`.

## Non-goals (Out of scope)

- **Agent-specific logic** — codex/claude binary command builders, hook/trust, telemetry stay in the plugins. The host only attaches an arbitrary command.
- **`terminal launch` (creating a new workspace)** — an agent convenience command, out of scope.
- **Soft input independence (no pollution of pending input)** — currently attached with a plain `surface.send` (the ADR only sets the direction).

## Acceptance Criteria

- [x] Given workspace `<ws>` When `terminal.spawn{parent=P, command}` Then a child terminal is created and registered in the registry, and `occupancy_of(child)==Soft` · `holder.parent==P` · `attached=false`.
- [x] Given an occupied child C When `terminal.kill` Then `occupancy_of(C)==None` + the surface is closed.
- [x] Given a registry with a dead child left over When `terminal.children` Then it is removed from the list by reconcile.
- [x] Given an arbitrary existing surface (including a plain terminal tab not made by spawn) When `terminal.adopt{surface=P, target}` Then `occupancy_of(target)==Soft` · `holder.parent==P` · it appears in the `terminal.children` list.
- [x] Given an already registered child or a hard-occupied target When `terminal.adopt` Then an error is returned + the registry is unchanged.
- [x] Given an occupied child C When `terminal.release` Then `occupancy_of(C)==None` + it disappears from the `terminal.children` list + the surface (tab) is still open (not closed).
- [x] Given an unregistered child index When `terminal.release` Then an error is returned.
- [x] Given another surface unrelated to C is hard-occupied When `terminal.release{child=C}` Then that hard occupancy is unaffected.
- [x] Given a running child C When `terminal.state{surface=C}` Then `{"state":"active","surface_id":C}`.
- [x] Given a child C terminated by `terminal.kill` When `terminal.state{surface=C}` Then `{"state":"exited","surface_id":C}` (not `"active"`).
