---
id: ISSUE-261118-0616-01
kind: issue
category: bug
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: four Rex fake-host contract tests pass or fail on the behaviour they check, not on how fast each fake `rex` call answers on a loaded machine; the production Rex time limits do not change
terms: [Host]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p8-tests
---

## Agent Brief

**Outcome and scope:** These four tests in `tests/contract/rex.rs` give the same result whatever time each fake `rex` call takes to answer (maintainer decision Q337): `a_timed_out_or_unparseable_rex_ls_makes_the_listing_incomplete`, `a_timed_out_or_unparseable_block_ls_keeps_the_other_sessions`, `a_directory_published_meanwhile_is_used_as_it_is`, and `a_sweep_during_publication_keeps_the_new_directory`. Tests only; the scope is the fake `rex` (`tests/contract/fake_rex.py`), the test harness, and test-only (`test-fixtures`) controls in `core/src/host/rex.rs`.

**Required sources:** `tmux-tools:PRD-261112-2108-01#p4-registration` (the production 2-second address-check limit of P4.1 and the incomplete-listing rule of P4.3 that the tests check); `tmux-tools:PRD-261112-2108-01#p8-tests` (the cases these tests own, and the deterministic publication/sweep interleave).

**Current observable:** On a loaded machine the two listing tests fail with exit 1 and stderr `host rex (selected by REX_BLOCK) is unreachable: rex server status: no answer within 1s`: the 1-second test call limit applies to every fake call, not only to the call that the case makes time out. When every fake call is 2.5 s slow, the sweep test fails: the sweep's 2-second address check of the publishing server's directory times out.

**Required change:** In each of the four tests, the call that a case makes time out reaches its time limit at any fake speed, and no other fake call of the test reaches a time limit, whatever its speed; the implementer chooses the method. Any test-only control that changes a time limit exists only in a `test-fixtures` build and applies only in the cases that set it. In a build without `test-fixtures`, the `rex` call limit (120 s) and the P4.1 address-check limit (2 s) are unchanged.

**Acceptance:**
- [ ] Given a fake `rex` that delays each call by 3 s before it answers, when the four tests run, then all four pass. Observable in the result of those four tests with that delay applied (the delay control is part of this issue).
- [ ] Given the same delay, when each case of the two listing tests runs, then `list` exits 0, stderr contains `list incomplete: rex`, and the rows have the statuses that the tests already require. Observable in the same tests.
- [ ] Given a production change that makes a timed-out or unparseable `rex ls` or `rex block ls` produce a complete listing, when the two listing tests run, then they fail. Given a sweep that deletes a directory whose publication is in progress, when the sweep test runs, then it fails. Given a production change that, when `rex/<instance_id>/` appears after the missing-directory check and before the rename, fails the spawn, puts the record outside the existing directory, changes that directory's `server-address`, or leaves a `.init-` directory, when `a_directory_published_meanwhile_is_used_as_it_is` runs, then it fails. Observable as a failing `cargo test` run for that change.
- [ ] Given a build without `test-fixtures`, when the backend runs a `rex` call or a P4.1 address check, then the limits are 120 s and 2 s. Observable in the unit tests of `core/src/host/rex.rs` or by inspection of the constants.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes with no delay, and every other fake-`rex` test keeps its current limits and passes unchanged; the four tests pass with the 3 s delay of the first acceptance item. Test-only change; no release.

**Out of scope:** Other Rex tests that use wall-clock limits, including `the_sweep_deletes_a_directory_whose_address_check_times_out`. Changes to the production Rex time limits or listing behaviour.

## Triage Notes

- 2026-11-18: draft → needs-triage (mint check passed) → ready-for-agent (dispatch readiness passed).
