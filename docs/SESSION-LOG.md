# SESSION-LOG

How we got here, newest first. Decisions carry their reasoning; mistakes carry
their mechanism, because the mechanism is what generalizes.

## 2026-10-08 to 2026-10-11: build to v0.1

**What was asked.** Build orin v0.1: whole-disk instant file search with a
daemon, CLI, TUI, MCP server, benchmarks against find/fd/rg, and a website.
Then: "drive this one home, get orin to be 1000% done as a better search tool
for AI agents and humans than its competitors."

**Decisions and reasoning.**

- TUI, MCP, and the CLI share one query language (ADR 0004 consequences):
  three surfaces with three grammars would be three products.
- Benchmarks got self-validating rows after two benchmark lies were caught
  (ADR 0005). The reasoning: a harness measures whatever it is fed, so the
  harness must be able to refuse its input.
- Single binary (ADR 0001) came from the owner's review question: "wait does
  this need 3 things installed? lol?". The count of executables is the first
  thing a reviewer says out loud, so it is a product metric.
- Windows-only (ADR 0002) came from the owner mid-benchmark: "nope windows
  only is all gucci". The spec's cross-platform line was superseded, not
  ignored.
- Path resolution (ADR 0006) came from measured evidence: two independent
  probe runs said ~6.5 minutes for 25,926 entries. The comment on the slow
  code said "slow but correct for tests"; production called it per entry.

**Corrections received, verbatim.**

- "the queries should be more realistic? or do you think they are," - the
  synthetic classes measured machinery, not human searches. Realistic classes
  (multiword, dates, camera names, deep paths, noise dirs) were added beside
  them, not instead of them.
- "Why do you need a github app" - the git-connected deploy model needed it;
  the domain's existing pattern (Cloudflare DNS + external origin) did not.
  The deployment was re-architected to the pattern the owner already uses.
- "should also include a proper agents.md and skills etc dont you think" -
  the project shipped code without its context layer. This layer is that
  answer.

**Mistakes and their mechanism.**

- A test stub (`lookup_path`, linear scan) became production because nothing
  in the build distinguishes "correct for tests" from "fine for production".
  Mechanism: performance debt hides behind comments. Fix: measure startup.
- A benchmark timed error exits as speed (System32 `find.exe` answered the
  `find` arm). Mechanism: bare tool names resolve differently under
  CreateProcess than under the shell. Fix: pin and identity-gate binaries.
- A benchmark scored empty searches as fast searches. Mechanism: a harness
  that times whatever it is given. Fix: preflight refuses zero-match warmups.
- Readiness polled "entries > 0", so queries ran against partial indexes and
  match counts drifted run to run. Fix: poll the daemon's `ready` state.
- A shared git index let one agent's commit absorb another agent's staged
  files under a wrong message. Fix: pathspec commits, recorded in
  `docs/adr/0005` adjacent practice and in every agent skill.
- Two benchmark runs disagreed with the query arms on daemon readiness: the
  probe harness waited on first entries while the daemon could not answer
  status until the scan finished. Mechanism: the instrument was measuring its
  own polling loop. Fix: timeline logging per poll, then root-cause.

**Where it ended.** All five surfaces exist; perf is measured and replicated
(8-14 ms warm p50 vs 40-60 ms for the walkers); the site is live at
https://orin.hjermitslev.dev. Deliberately open: the README's measured table,
1M/3M scale probes (the spec's target table), and the benchmark-data page on
the site.
