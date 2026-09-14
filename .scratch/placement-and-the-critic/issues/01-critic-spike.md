# 01: Spike — critique an archived run by hand

**What to build:** Nothing in the product. Point an ordinary Claude Code session at a completed run's archive directory and ask it to critique the run under the Critic's intended rules — report only what the fleet *did*, cite a timestamp, file and line from the archive for every finding, and never judge whether the code is correct.

The unknown this answers is the one the self-improvement arc names: a judge with no answer key is warned to converge on "solid work, maybe more tests." If that happens here, ticket 08 should not be built, and learning it costs one session instead of a package. `building.md` §4 requires this shape — anything touching an unknown gets a throwaway first, and the measurement goes in the docs.

Run it against at least two archived runs, ideally one that went well and one that did not. The archive is already written to be read by an agent, transcripts included (D-059), so nothing needs to be exported or reshaped.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] At least two completed runs critiqued, from their archives alone
- [ ] Findings recorded as a measurement note under the project's notes directory, version-stamped like every other note there
- [ ] The note states plainly whether the output was worth reading, including if the answer is no
- [ ] Every finding in the note carries a citation into the archive; any finding that could not be cited is listed separately as evidence of the failure mode
- [ ] The prompt that produced the best results is captured verbatim in the note, ready to become the Critic's brief
- [ ] The note names the categories that actually produced findings (idle time, uncited done-claims, blocks with no performance criteria, contention over one file, unanswered messages) and which produced nothing
