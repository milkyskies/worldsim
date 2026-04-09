---
paths:
  - "src/**"
---

# Module Documentation

Every module file (`src/agent/brains.rs`, `src/agent/mind.rs`, etc.) must have a `//!` doc comment at the top with:

1. One-line description of what the module does
2. `Reads:` — what data/components this module consumes
3. `Writes:` — what data/components this module produces
4. `Upstream:` — which modules feed into this one
5. `Downstream:` — which modules consume this one's output

Keep it under 6 lines. Describe connections, not implementation.

When modifying a module, check that its `//!` header still reflects reality. Update if not.

## SimEvent Observability

Any system that produces a meaningful state change (brain decision, action lifecycle, knowledge change, relationship change, conversation event, emotion trigger, death, perception, etc.) **must emit a `SimEvent`** via `EventWriter<SimEvent>`. The `SimEvent` enum lives in `src/agent/events.rs` and is the foundation for all logging, tracing, and debugging tooling.

When creating a new system:
1. If your system produces an event type not yet covered, add a variant to `SimEvent` in `src/agent/events.rs`
2. Emit events at the relevant points in your system using `EventWriter<SimEvent>`
3. The `//!` header's `Writes:` line should include `SimEvent` if applicable
4. Every variant must carry `tick: u64` and the relevant `agent: Entity` for filtering

Without this, the structured event log (`--log`), decision trace (`--trace`), CLI inspection (`--inspect`), and TestWorld print methods cannot observe your system.
