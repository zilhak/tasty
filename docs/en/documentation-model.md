<!-- source-hash: 650fc41f4129 -->
# Tasty Documentation Model

> This document is the central rule for **how every Tasty design/specification document is divided, where each belongs, and how they connect**. Read it before creating a new document or editing an existing one. The *rationale* for this taxonomy is [ADR-0006](adr/0006-docs-taxonomy-behavior-first.md).

## 1. Core principle — behaviour first, screens second

> Tasty's full identity is in [`identity.md`](identity.md). This section covers only the axis *directly relevant to document structure* (headless, behaviour-first).

Tasty **runs headless**. So the truth of a feature is its *internal behaviour*; the *screen* is merely that behaviour projected for a human user.

- **Behaviour document** = what the feature does internally. Valid even headless. **First priority, the parent.**
- **Screen document** = how that behaviour appears on screen. **Second priority, the child.**

This axis is the same axis as Tasty's identity principle, **separation of user and agent actions**:

| | Behaviour doc (1st) | Screen doc (2nd) |
|---|---|---|
| Identity | Internal behaviour (headless-valid) | Visual projection of the behaviour |
| Action axis | Agent actions (CLI/IPC) | User actions (keys/mouse) |
| Relationship | Parent | Child (subordinate) |

**1 behaviour doc : 0..N screen docs.** A headless-only feature has zero screens. A behaviour projected onto several screens has N.

## 2. Document map (all categories)

The docs split into two groups. The **behaviour-first taxonomy of §1 applies only to A (product specification)**. B (development and operations guides) is an independent category unrelated to the screen/behaviour axis.

### A. Product specification — "what tasty is"

| Kind | Location | What | Owner / changes |
|------|------|--------|-------------|
| Behaviour doc | `docs/features/<f>/index.md` | Internal behaviour (1st priority) — **provided by the host** | docs / free |
| Screen doc | `docs/features/<f>/screens/<s>.md` | Visual projection (2nd priority) | docs / free |
| Bundled plugin | `docs/plugins/<id>/index.md` (+ `screens/`) | Behaviour and screens **provided by a plugin** (same structure as features, only the provider differs) | docs / free |
| Cross-cutting rules & flows | `docs/design/{policies,flows,systems}/` | Rules/flows shared by several features | docs / free |
| Terminology | `docs/concepts/` | The ubiquitous language | docs / free |
| Rationale (ADR) | `docs/adr/` | Why it was decided that way (including deferred decisions, alternatives, and re-review triggers) | docs / immutable once Accepted (supersede instead) |
| Visual truth | `design-system/…` (vendored) | Pixels / tokens / components | **claude design** / only via design-request |

**Visuals are never restated in docs.** A screen doc lists the element inventory and the "behaviour state → visual" mapping only; pixel and token values are linked to `design-system/`. Copying them creates two truths that drift.

### B. Development and operations guides — "how to handle tasty"

| Kind | Location | What | Audience |
|------|------|--------|-----------|
| **Developer guide** | `docs/dev-guide/` | **How to develop tasty — build, commit, release, plugins, i18n, error handling, GPU, debug IPC, self-verification, …** | AI agents developing tasty |
| Architecture | `docs/architecture/` | Crate structure / data flow / invariants | AI agents developing tasty |
| Self-verification | `docs/ai-verification/` | UI and rendering verification procedures | AI agents developing tasty |
| Agent guide | `docs/agent-guide/` | How to operate tasty over IPC/CLI (shipped as a release asset) | the user's AI agents |
| Installation | `docs/installation.md` | Installation per OS and architecture | users / agents |

> B derives from code and process and stays largely valid regardless of the claude design adoption. It is therefore **a review-and-correct target, not a rewrite-from-scratch target** (the re-organisation procedure is in [`index.md`](index.md)).

## 3. Folder structure (nested)

```
docs/features/<feature>/
  index.md            # behaviour doc (internal behaviour, 1st priority)
  screens/
    <screen>.md       # screen doc (2nd priority) — 0..N files
```

- Headless feature → no `screens/` (the absence of a screen shows in the structure).
- Multiple screens → several files under `screens/`.
- Templates: [behaviour](features/_feature.template.md) · [screen](features/_screen.template.md).

## 4. Linking — composite screens connect by "mention" only

Composite screens where several features meet (the sidebar, MainView, the settings window, …) **describe only their own area and merely link** the document for any element delegated to another behaviour or window. No embedding, no copying.

Example — the sidebar screen doc:

```
## UI element inventory
- Top: icon / logo / collapse-button area
- Middle: workspace area (all remaining height)
- Bottom:
  - Tools button      → see features/tools-menu/
  - Plugins button    → see features/plugin-system/screens/plugins-window.md
  - Settings button   → see features/settings/screens/settings-window.md
```

The sidebar doc does not say "what is in the tools menu" — it puts a link next to the button description and nothing more.

## 5. design ↔ code (collaboration with claude design)

Design is a claude design deliverable (`design-system/`), and claude code **never edits it directly**. `design-system/` owns the visual truth; docs **only link** to it (no restating).

When a design needs to change, do not touch the source first — **request the change from claude design**, then **receive the changed design and re-apply it**. Absorb what comes back into docs per the placement rules in §6 (behaviour → features, visuals → a link to design-system, rationale → ADR). The concrete workflow for submitting and retrieving requests is defined in [`dev-guide/design-change-workflow.md`](dev-guide/design-change-workflow.md) (request document → draft → reconciliation loop).

## 6. Writing rules in brief

- New screen / behaviour → absorb into the behaviour and screen docs of the relevant `features/<f>/`. Create the folder if it does not exist.
- **Put only content that fits the document kind (placement rule).** Internal implementation (files, function call sites, feature gates, behaviour wiring) **may be written — but its place is dev-guide or the behaviour doc (internal behaviour)**. `agent-guide` (usage) covers only *what an agent can do with tasty*, so implementation detail does not go there (not because the implementation is *wrong*, but because it is *unnecessary in that section*). Build/roadmap status (`Phase …`, `to be implemented`, `migration status`) is transient and goes nowhere (current state only).
- Do not write visual values or tokens; link `design-system/`.
- Do not argue a decision at length in the body; pin it in an ADR and link.
- Composite screens connect by mention/link only.
- When a design change is needed, do not touch the source first — request it from claude design and re-apply the changed design (the concrete workflow is in [`dev-guide/design-change-workflow.md`](dev-guide/design-change-workflow.md)).

## Related

- [ADR-0006 — Documentation taxonomy: behaviour first](adr/0006-docs-taxonomy-behavior-first.md) (rationale)
- [features/index.md](features/index.md) (behaviour and screen catalogue)
- [dev-guide/design-change-workflow.md](dev-guide/design-change-workflow.md) — the design-change workflow (request document → draft → reconciliation loop)
