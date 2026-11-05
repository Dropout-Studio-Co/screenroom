---
title: Linux CI player worker tests segfault or hang when run in parallel against Ubuntu libmpv
author: Beyourahi
date: 2026-11-05
branch: main
cost: { minutes: 1, tokens: 1415 }
id: l-a2458e79
scope: repo
status: active
src: agent
---

## Decided
- `ci-linux.yml` runs the core and player tests with `--test-threads=1`, and each test step has `timeout-minutes: 30`. After the change the Rust tests job passed in about 3 min (run 42058975672).

## Watch out
- Before the change, `screenroom-player --test worker` crashed with SIGSEGV in one run and hung a release run's test job for about 2 hours in another. It passes some of the time, so one green run proves nothing.
- `macos-latest` (arm64) jobs can be dropped with "The job was not acquired by Runner of type hosted" during GitHub capacity shortages. That job ends as cancelled and the cause is not in this repo.
