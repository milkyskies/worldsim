# Make the sim actually interesting

Started 2026-05-09. Working name: "make-sim-feel-real" arc.

## The problem we're solving

Agents have a deep cognitive stack (3 brains, GOAP, MindGraph, theory of mind, ~14 drives, body-with-channels, etc.) but the *world* and the *vocabulary* are too thin for any of it to matter. People "kinda just exist and eat and shit." Survival is too easy. Nobody has history. Nothing visibly produces anything. There's exactly one buildable recipe (Campfire). It's a perfectly tuned engine in first gear.

## What we're aiming for

RimWorld-level texture, Dwarf-Fortress-level emergent storytelling. Concretely: a day in the sim should look like 40% productive / 20% social / 15% rest / 15% casual / 10% idle (the goal in #395 RISOU). Agents form group goals to survive. Deaths matter forward in time. Places acquire meaning. Cultures emerge.

## Phases

Durable copy of this lives in epic #395 body — keep that in sync if this drifts.

### Phase A — Stuff to do, lightly hardened survival (NOW)

Goal: real production chains, more verbs, food no longer infinite. Day-to-day pressure becomes non-trivial.

- #325 Cooking ✓ shipped
- #270 Plant regrowth ✓ shipped
- #269 Expand human action registry ✓ shipped (Mourn / Celebrate held for #88 death-and-grief)
- #323 Lean-to / House ← **next lead**
- #324 Storage chest / food cache

### Phase A½ — Cognitive primitives (pull forward when Phase A wraps)

These are not new behaviors. They are *primitives* that, once shipped, collapse multiple Phase D texture issues into thin opt-ins on existing systems. Pull them forward instead of shipping equivalent behavior as a one-off in each texture issue.

- **#735 Anticipation** (just filed) — drives can fire on forward-projected state. Subsumes #423 anticipatory food, #733 campfire pre-sleep top-up, generalizes to: build shelter before dusk, stockpile before winter, prepare for journey, predict-and-act on any deterministic future state. Closed-form predictors, not forward-simulation.
- **#736 Other-regarding drives** (just filed, blocked by #540) — drives can read another agent's state, scaled by affection. Subsumes protectiveness, kin-feeding, friend-defense, parental rage, vicarious aspirations. Composes with #735 for the dramatic anticipatory caregiving moments.
- **#540 Affective ToM** (sub-issue of #517 Psychology Overhaul) — store last-observed mood per known agent. Hard prereq for #736.
- **#535 OCC 22 emotions** (sub-issue of #517) — replaces Ekman 6 with full 22-emotion enum. Primitive that every emotion-generating system reads. Pulling this forward is what makes Gratitude / Pity / Resentment / Hope / Relief / etc. expressible at all.
- **#538 Appraisal function** (sub-issue of #517) — full OCC appraisal variables drive emotion generation from personality + values + outcome, replacing hardcoded action→emotion mappings. Without it the OCC enum is just labels.
- **#669 Layered knowledge inheritance** — cultures, species defaults. The "humans-need-shelter-at-night" cultural prior lives here, not in code. Once shipped, every new texture issue can express its priors as data, not as branching.
- **#532 Social and role identity** (sub-issue of #517) — "I am the village hunter / mother / elder." Primitive that role decomposition (#411) and group dynamics (#76) both ride.

This wave is sub-issues plucked from larger epics (#517 Psychology Overhaul, plus #669) — we do *not* ship all of #517, just the primitives whose downstream leverage is highest. The rest of #517 stays in Phase E.

### Phase B — Group goals, real coordination

Goal: agents stop building five separate campfires; they help each other build one shelter.

- #410 PlanEntity as ECS entity
- #411 Role decomposition for shared plans
- #412 Verbal plan-ref coordination
- #413 Perceptual plan-ref coordination
- #414 Ask-to-confirm peer activity
- #415 Collaborative commitment inputs

### Phase C — Cross-agent emergent structure

Goal: settlements, reputation, named places, group norms, sacred sites stop being scenery and start being *recognized* by the sim.

- #705 CollectiveBeliefView substrate ← only real hard dep in this group
- #710 Settlement as emergent belief cluster
- #69 Reputation / gossip view
- #76 Group dynamics / norms view
- Named places view (sub-piece of #704)
- Sacred sites view (sub-piece of #701)

### Phase D — Texture and depth (opportunistic)

Pulled in as they thematically fit. No rigid order.

- #701 Religion, ritual, sacred — pairs with #710 settlements + #88 death/grief
- #702 Property, theft, economy — only meaningful once #710 is real
- #704 Place meaning / oral history — pairs with named places + #69 gossip
- #703 Expressive culture — needs nothing structural; ship anytime as content drop
- #706 Disease / contagion — alternative survival pressure orthogonal to weather/seasons
- #707 Heirlooms / inheritance — needs death + reproduction to land first
- #708 Habits / quirks / superstitions — falls out of #550, ship near #517
- #709 Tool durability and repair — flip on once Phase A crafting exists

### Phase E — History layer (long arc)

Held until A-C are solid. Backstories without daily-life machinery to act on are flavor text.

- #517 Psychology Overhaul — backgrounds (#527), aspirations (#529), autobiographical memory (#549), emotional memory conditioning (#550), self-discrepancy (#530), OCC emotions (#535+), regulation (#546), coping (#547)
- #310/311 Reproduction + #312/314 Genetics — generational continuity

## Phase A — current slot

Wave 1 ✓: #325 Cooking + #270 Plant regrowth merged.
Wave 2 ✓: #269 action registry merged (Mourn / Celebrate held for #88).

Wave 3 (now) — sequential, not parallel:

| Slot | Issue | Why solo |
|---|---|---|
| 1 (lead) | **#323 Lean-to / House** | The remaining drive-to-plan piece in Phase A. Touches drive_registry, cns, urgency, proposal, rational. |

**Why no parallel slot this wave:** the only obvious parallel candidate is #324, but it edits the same drive-to-plan pattern files as #323 (different drive, same files). Line-level conflicts are tolerable but pointless when #324 will be a fast mechanical follow-up. Phase D candidates (#703, #706) also want to add their own drives, so they collide on the same files too. Lead solo, then chain #324.

### Wave 4 (after #323 merges)

- **#324 Storage chest / food cache** — should be near-mechanical: copy #323's drive-to-plan plumbing, swap the drive semantics to food-security, swap the world entity for a chest. Bigger work is the Deposit/Take wiring against `ItemSlots` (#139), but that substrate already exists.
- **Mourn / Celebrate** — file once #88 lands; that may itself be a bigger arc, so don't block Phase B on it.

After Wave 4, **Phase A is done**. Move to Phase B (#408 Phase 2/3: PlanEntity → role decomposition → verbal/perceptual coordination → joint-commitment inputs).

## Rules of engagement (planner mode)

- Real `glb dep` edges only when an issue genuinely *cannot start* until the blocker ships. Preference ordering lives here, not in glb. (Only real dep wired so far: #710 → #705.)
- Each issue claimed via `glb update <num> --claim` before work starts.
- Re-derive next slot from this file + `glb ready` + `glb path` after every merge — don't follow this list blindly if the situation has changed.

## Open questions / things to revisit

- Tuning #270 plant regrowth rate — start where? (RimWorld berries respawn over ~5-7 game-days; biology clock ÷6 ≈ same.) Propose numbers when we get to it, don't auto-apply.
- Spoilage for cooked food — in scope for #325 only if already wired; otherwise defer to a separate Phase A issue.
- Whether to flip #271 seasons / #272 weather along with #270 — current call is no, don't over-tune. Revisit after Phase A ships and we see real behavior.
