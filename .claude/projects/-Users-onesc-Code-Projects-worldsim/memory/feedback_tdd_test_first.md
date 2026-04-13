---
name: Write failing test before fixing bugs
description: TDD approach - write a test that fails due to the bug, then fix the bug
type: feedback
---

When fixing bugs, write the test FIRST (before touching production code). The test should fail because of the bug. Then apply the fix and confirm the test passes.

**Why:** This is the TDD workflow the user expects. It also proves the test actually validates the bug rather than just testing the fixed behavior.

**How to apply:** For every bug fix:
1. Read the buggy code
2. Write a test that exercises the broken path and asserts the correct behavior (it should fail against the current code)
3. Commit the failing test
4. Apply the fix
5. Confirm test passes
6. Ship
