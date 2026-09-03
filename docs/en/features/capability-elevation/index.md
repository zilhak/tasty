<!-- source-hash: 617603b6243c -->
# Capability elevation & audit

- **Status**: Implemented
- **Actors**: AI agent (requests) · local user (approves) · operator (queries the audit)
- **ADR**: none
- **Code**: the dispatcher permission gate (`handler.rs`), `src/adapters/ipc/audit.rs`, the session/elevation handlers
- **Screens**: the capability_elevation approval popup
- **Method list**: [reference/api](../../reference/api.md#plugin-관리-plugin-local-only)

## Purpose

When a plugin/agent makes an IPC call the host enforces permissions, and when they fall short it **obtains elevation from the user immediately via a popup** and **persists denied calls to the audit log**. A runtime permission model layered on top of the manifest permissions ([plugin-permissions](../../dev-guide/plugin-permissions.md)).

## Internal behaviour

### Three axes

1. **Permission evaluation** — `method_meta` declares the required permissions per method. All must be contained in the caller's permission set to pass.
2. **Capability elevation** — when an agent is refused for lack of permission, an automatic popup.
3. **Audit log** — every IPC (allow+deny) persisted at the dispatcher's single entry point.

### Agent session permissions

A child launched via claude.spawn etc. is issued a token with `session.issue` (base permissions = a subset of the parent's permissions, escalation forbidden). The child attaches `TASTY_SESSION_TOKEN` as the envelope `session_token` on every call → the host builds `CallerContext::Agent`. Invalid/expired/revoked tokens yield `-32001` (no Local fallback — forgery defence). Additional runtime grants go through `plugin.grant_agent_permission` (TTL possible, a slot separate from base).

### Elevation flow

When an agent is refused for lack of permission, the host (if no Pending exists for the same (agent, permission)) automatically issues `approval.request{kind=capability_elevation, choices=[approve, approve_permanently, deny]}`. The denial response carries `{approval_id, permission, method}` in `error.data` → the agent polls `approval.await` → per the user's choice a temporary (TTL)/indefinite grant is applied → the agent's retry passes. An agent can also issue it **in advance** with `plugin.request_permission` (without waiting for a denial). The mechanism sits on top of [human-handoff](../human-handoff/index.md).

### Audit log

Record: `ts_ms, seq, caller_kind(local/internal/plugin/agent), caller_id, method, decision(allow/deny), reason?, workspace_id?`. Persistence key `tasty.audit.{ts}.{seq}` (global, lazily evicted on query). `seq` is a monotonic counter shared with telemetry.

**Only denies are recorded.** Allows are not stored regardless of method — in a polling agent workload, allows created permanent records at 14 per second and became the largest inflow into `memory.db` (371,936 rows in an 18-hour run, 0 of them denies). So this log is **effectively an empty table in normal operation**, and it has no use for tracing after the fact "what the agent actually did with its approved permissions". Conversely, every deny of any method is kept, so tracing permission-denial incidents is unchanged. The decision's rationale, alternatives and revisit conditions: [ADR-0085](../../adr/0085-ipc-log-retention-bounded.md).

Elevation itself does not depend on this log — the approval history is persisted separately under `tasty.approval.*`, and the elevation issue trigger is the deny path too.

Retention: follows the policy common to the three observation logs (`adapters::ipc::log_retention`) — a 50-hour + 50,000-record cap, enforced at both boot and runtime.

## Interface

- **AI agent**: `session.issue` (AgentManage), `plugin.request_permission` (Approval), `plugin.list_agent_permissions` (readonly).
- **Operator/CLI (local-only)**: `plugin.{grant,revoke}_agent_permission` · `plugin.audit_{query,summary,follow,clear}` (`tasty plugin audit-query/summary/follow/clear` — follow polls on the CLI side).

## Related

- [dev-guide/plugin-permissions](../../dev-guide/plugin-permissions.md) — permission tokens/manifest · [human-handoff](../human-handoff/index.md) — the approval mechanism
- [identity](../../identity.md) — user/agent separation
