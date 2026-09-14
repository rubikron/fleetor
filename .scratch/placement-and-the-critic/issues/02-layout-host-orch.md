# 02: Layout and Host, with orch placed through them

**What to build:** The orchestrator pane comes up through a new placement module instead of through the improvised sequence in the orchestrating module. Its working directory, configuration seed, write-guardrail roots, spawn notices and command are all decided in one place, against a layout value it is handed rather than against the operator's real home directory read mid-call.

Two values arrive with it. A **layout** says where this fleet lives on disk — built from the operator's home in production, and over a scratch directory in a test. A **host** says what this machine has — the operator's Rust toolchain, the built `fleet` binary, the worker API key, the mission harness roots, and the stand-in pane override. The host is discovered per spawn, exactly as those things are resolved today, so behaviour is unchanged.

Workers and the evaluator keep the old path for now; both still spawn correctly. This ticket is the tracer bullet that proves the seam works end to end for one pane kind.

From the design prototype — the shape, not a working demo:

```
enum PaneSpec { Orch, Worker(slot), Evaluator, Critic { run } }

place(spec, &layout, &host, target, &context) -> Result<Placed>

struct Placed { command, notices, gauge }
```

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] The orchestrator spawns and reaches its prompt exactly as before; no operator-visible change
- [ ] Placement performs no process-global reads — no home directory, no environment variable, no configuration path resolved inside it
- [ ] The layout has two real constructors, one from the operator's home and one over a caller-supplied directory
- [ ] The host has two real constructors, one discovering the machine and one describing a bare machine
- [ ] Placing the orchestrator against a scratch layout writes its configuration seed, its guardrail policy and nothing outside that directory
- [ ] A test asserts the orchestrator's guardrail roots and its returned notices, driven entirely through the placement seam
- [ ] Notices are returned as data rather than written to the log from inside placement
- [ ] The existing suite is green with no test deleted or weakened
