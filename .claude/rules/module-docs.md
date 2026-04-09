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
