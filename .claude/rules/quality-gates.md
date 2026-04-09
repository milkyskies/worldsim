# Quality Gates (worldsim)

**This rule overrides the default Rust quality-gate pattern for worldsim.** Worldsim has a heavy compile cost (Bevy, ~900 dep crates) — running the full check trinity locally takes 5-15 minutes per push. CI runs it on a fast runner (`ubicloud-standard-8`) with caching, so local runs are wasted work.

## Local pre-push gate

Before pushing, run **only fmt**:

```bash
cargo fmt
```

Do **NOT** run `cargo clippy` or `cargo nextest run` locally as part of the pre-push gate. They duplicate what CI already does, much more slowly.

## Verifying your code works

If you want to sanity-check your changes locally before pushing:

- **Compile check**: `cargo run` (you'd be doing this anyway to play the game)
- **Run a specific test you just wrote**: `cargo nextest run -E 'test(<your_test_name>)'` — note this still pays the full Bevy compile cost, but skips running unrelated tests
- **Skip everything**: just push and let CI catch issues

## Trust CI

CI is the source of truth for clippy + full test suite. If CI fails, fix locally and push again. Do not try to "pre-validate" by running the full trinity locally.

## `/ship` skill — Step 2 override

When `/ship` runs Step 2 (Quality gates), the gate for worldsim is **only `cargo fmt`**. Skip clippy and skip the full nextest run. Then proceed straight to push and the CI loop. Trust CI failures as the authoritative signal.

## Why this is different from other Rust projects

Other Rust projects in this kit (floe, argus) compile in seconds and the full local trinity is fast. Bevy is the outlier — its dep tree is large enough that the only practical workflow is to defer heavy checks to CI.
