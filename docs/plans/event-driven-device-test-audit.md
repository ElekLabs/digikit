# Follow-up — Audit device-test waits

Status: planned. Use a separate branch and warm worktree based on the
[firmware view observer checkpoint](dsp-development-workflow.md). Keep this
audit off the Phase 8 reference-capture branch.

## Goal

Make existing device tests fail on the condition that matters. Replace fixed
delays and pixel-based readiness checks where firmware events or structured
readbacks can prove the same condition. Do not expand device coverage or
change what a test asserts just to change how it waits.

## Approach

1. Start with the [device test inventory](ledgers/device-test-inventory.md)
   and the runtime acceptance manifests. Find fixed waits, pixel checks
   used for navigation, and cases that wait only for `INPUT_TAKEN`. Record
   the file, operation, success condition, current wait, proposed signal,
   affected device halves, and whether the change removes a real failure
   mode or wasted time.
2. Prefer existing `RuntimeEvents` kinds such as `SETUP_DONE` and
   `PARAM_CHANGE` where they cover the exact state. Use the opt-in
   `VIEW_CHANGED` stream for a mapped top-of-stack transition. Start from
   a fresh cursor and the observer snapshot. Use an authoritative state
   readback when an event cannot distinguish a reused dialog or page.
   `INPUT_TAKEN` proves delivery, not a successful state change.
3. Start with one menu-navigation test and one value-edit test if the
   inventory finds good candidates. Use shared harness helpers and prove
   wait mechanics with host fakes. Run only the named DT2 and DN2 device
   halves for the behavior that changed. Keep fixed durations that are
   part of the workload, and keep pixel assertions in rendering tests.
4. Report the candidates kept, changed, and blocked on missing readbacks.
   Treat new event sources and Syntakt observation as separately scoped
   work. Do not silently skip tests or extend their deadlines as a fix.

## Exit check

The selected tests have an explicit success condition and a useful timeout
message. The named device halves pass. The report says which timing or
framing assumptions were removed and why the remaining fixed waits stay.
