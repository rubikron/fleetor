# Rotating a WAL-mode event log — what survives the move

Measured 2026-08-07 against the live `~/.fleetor/_shell/state.db` and reproduced
synthetically by `examples/run-rotation-spike/rotate.sh`. SQLite 3.x as shipped
with macOS 15 (Darwin 24.6.0). The question WP-11 had to answer before writing
`runs.rs`: **when the fleet start archives the previous run's database, which
files does it have to take?**

## The finding

**Moving `state.db` on its own destroys the run it is archiving.** Not "loses
recent events" — destroys. The archived file does not contain the schema:

```
$ sqlite3 a/state.db "SELECT COUNT(*) FROM events;"
Error: in prepare, no such table: events
```

That is the *whole* log gone, migrations included, from a file that still looks
plausible on disk.

## Why

A WAL-mode database only moves pages from `state.db-wal` into `state.db` at a
checkpoint. SQLite checkpoints on the last connection's clean close — and the
fleet's last connection frequently does not get one. A force-quit, a panic, or
a `kill -9` of the Tauri process leaves everything in the `-wal`.

The live specimen, taken from the operator's machine before any code was
written, is the case in its extreme form:

| file | size |
|---|---|
| `state.db` | 4,096 B — a header and nothing else |
| `state.db-shm` | 32,768 B |
| `state.db-wal` | **2,298,992 B** — 146 events and every migration |

The 4 KB file is the one an `fs::rename("state.db", …)` would have preserved.

## The four strategies, measured

Both the live specimen (146 events) and the synthetic one (500 rows) agree:

| | strategy | result |
|---|---|---|
| **A** | move `state.db`, leave `-wal`/`-shm` | **total loss** — `no such table: events` |
| **B** | `PRAGMA wal_checkpoint(FULL)`, then move `state.db` alone | all rows |
| **C** | move `state.db` + `-wal` + `-shm` together | all rows |
| **D** | `PRAGMA journal_mode=DELETE`, then move `state.db` alone | all rows, **and no `-wal` left to move** |

On the live specimen the checkpoint moved 558 pages and grew `state.db` from
4,096 to 184,320 bytes.

## What WP-11 does with it

**D, with C as the fallback.** Leaving WAL mode is documented to checkpoint and
delete the `-wal` as one operation; this note exists partly to check that claim,
and it holds — after the pragma, `state.db-wal` is gone and every row is in the
main file. So the archive step is a single pragma and a single `rename`, with no
window in which a half-moved run exists.

D also earns its place on the read side. An archived run is a plain
rollback-journal database, so it opens read-only from a directory containing
nothing but `state.db` — verified above, with no `-shm` present. A database left
in WAL mode wants to create a `-shm` beside itself even for readers, which is
exactly what a frozen archive should not be doing.

`FULL` rather than `TRUNCATE` in strategy B for no deep reason: they are
equivalent once the `-wal` is deleted, and `TRUNCATE` in a shell command trips a
destructive-SQL guard hook on this machine. B is not what ships.

Rotation **must not be able to fail a fleet start.** A history feature that
stops the app booting is a worse bug than the one it fixes. If the old database
cannot be opened at all, `runs.rs` falls back to C — move all three files
untouched — and if *that* fails it leaves the old files alone, announces a
`Warn` on the Activity feed, and boots. History is worth less than a fleet.

## The consequence nobody had noticed

Because `state.db` is durably 4 KB between runs while the real log lives in an
uncheckpointed `-wal`, **any backup, copy or sync of `~/.fleetor` that grabbed
`state.db` alone has been capturing an empty database this whole time.**
Nothing in the product does that today. It is written down here so the next
person who reaches for `cp state.db` knows what they would get.
