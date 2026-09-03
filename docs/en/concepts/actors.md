<!-- source-hash: 253dce0568d8 -->
# Actors

tasty is designed on the premise that several actors use the same instance **at the same time** (→ concurrency in [identity.md](../identity.md)). The three actors differ in *what they act through* and *which contract they follow*.

## Local user

The person using the tasty GUI directly on this machine. Input surface = keyboard shortcuts, mouse, native OS input. **Owner of focus.** Handles ordinary (unoccupied) surfaces/workspaces freely. Usually one per instance. **The only actor who can break an occupancy** (see the occupancy model below).

## AI agent

An AI that operates tasty to carry out its own work. Input surface = IPC methods / CLI subcommands, with targets addressed by ID. Several run concurrently and follow the **isolation contract** — side effects of their actions never touch user state (focus / closed-item history / selection). **By default they act without occupancy**, manipulating any target by ID (fire-and-forget `surface.send` / `surface.read`), but **they can take an occupancy (soft/hard) when needed** — e.g. the `terminal` command marks the child terminal it spawns with a soft occupancy (occupancy model below, [ADR-0040](../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md)).

## Remote user

A person connecting by attach from across SSH. **Behaviourally they are closer to an AI agent than to the local user** — they act through a *connection (the attach stream)* rather than direct GUI input, and they are not the owner of local focus. The decisive difference from an AI agent is **whether occupancy is mandatory**:

- Before doing anything, a remote user **must declare a hard (exclusive-claim) occupancy on a surface or workspace**, and can act **only within the occupied target**. The only things a remote user can touch are *occupied terminals/workspaces*.
- An AI agent can manipulate any target by ID without occupancy — occupancy is **optional** — whereas a remote user **must pass through the gate of occupancy**.

tasty has no remote protocol of its own and delegates to SSH — attach behaviour is in [`../features/remote-attach/`](../features/remote-attach/index.md), the mechanism in [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

## The occupancy model

An occupancy is **a persistent, visible relationship that an actor (remote user | AI agent) declares over a surface/workspace**. Unlike fire-and-forget operations such as `surface.send` / `surface.read`, it makes the fact "this target is currently being driven by some actor" explicit to the local user. There are two tiers, **visually distinguished** (terminal border colour: soft = green, hard = peach). Rationale and visual convention: [ADR-0040](../adr/0040-occupancy-soft-hard-tiers-agent-occupant.md).

### Soft occupancy

- **A display-only advisory marker.** It announces "this target is being driven by some actor and may be closed or change state at any time". **No write restriction** — the local user types and operates as usual. A cooperative signal, not an enforcement.
- Current consumer: the **child terminal** spawned by the `terminal` command (actor = the parent surface that spawned the child) → [`../features/child-terminal/`](../features/child-terminal/index.md).

### Hard occupancy

- **Exclusive + read-only.** Only the occupying actor operates; meanwhile **the local user and every other actor are read-only on that target** — they can only *watch* what happens.
- **Remote attach is the example of this tier** (a remote user works only under a hard occupancy) → [`../features/remote-attach/`](../features/remote-attach/index.md), mechanism [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md).

### Shared rules

- **Who can release**: in either tier, an occupancy is broken **only by the occupying actor itself (self-release) or by the local user (force-detach)**. Once broken, the target **returns to an ordinary surface/workspace**.
- **Exclusive (1:1)**: regardless of tier, **one target (surface/workspace) is occupied by one actor at a time**. Occupancy is *actor → target* 1:N and *target → occupant* 1:1. Simultaneous occupancy is impossible; for another actor to take it, the existing occupancy must be released first.
- **Multiplicity**: several remote users can connect at once, and one actor can occupy several targets at once.

## Summary

| | Local user | AI agent | Remote user |
|---|---|---|---|
| Kind | Human | AI | Human |
| Acts through | Direct GUI input | IPC / CLI | attach connection |
| Classification | User action | Agent action | **Agent-like + occupancy** |
| Occupancy | Not needed | **Optional** (soft/hard) | **Mandatory** (only inside a hard occupancy) |
| Focus | Owner | Never touches it | Not the owner of local focus |
| Force-detach of others' occupancy | **Yes** | No (self-release of its own only) | No (self-release of its own only) |
| Concurrent count | Usually 1 | 0..N | 0..N |

The separation "user action (direct local input) ↔ agent action (connection-based)" is tasty's soul, and every API design sits on top of it (→ [identity.md](../identity.md) §2.1).
