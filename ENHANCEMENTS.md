# Potential enhancements

Ideas considered and deliberately **not** built yet, with the reason and the signal that would
justify revisiting each one. This is a holding file, not a roadmap: nothing here is committed to.

The shape of the argument for deferring most of it: when these were first scoped, the store held
**zero tasks**. Reporting, mining, and dashboards are all query layers, and a query layer over an
empty store is unfalsifiable — it looks finished and tells you nothing. Capture came first
(`kind`, `tags`, `ticket`, plural `reviews`) because it is cheap and *expiring*: work logged
without a category cannot be categorised later by anyone who remembers what it was. Queries are
expensive and deferrable, because the data keeps.

Each entry therefore names a **trigger** — the observation that would mean it has stopped being
speculation.

---

## 1. Time-bounded reports

`taskforge report --from <date> --to <date>`: counts by `kind`, rollups by `tag`, closure
breakdown, over tasks whose `completed_at` falls in range.

**Why not yet.** The capture fields landed first, so the data has only just started accruing. A
six-month report is empty by construction for six months. At one person's volume,
`task list --json` piped through `jq` answers most one-off questions without new code.

**Cheap alternative today.**

```bash
taskforge task list --json | jq '[.data[] | select(.completed_at)] | group_by(.kind)
  | map({kind: .[0].kind, n: length})'
```

**Trigger.** You have written that `jq` line more than about three times, or you want the same
report on a schedule. At that point the question is stable enough to encode, and you will know
which fields it actually needs.

---

## 2. Categorical and date filters on `list`

`--kind`, `--tag`, `--ticket`, `--created-after`, `--completed-before`.

**Why not yet.** Same reason as above, and these are the plumbing a report needs rather than
something wanted on their own. Building them before the report means guessing which predicates
the report will want.

**Cheap alternative today.** `--status`, `--overdue` and `--stale` already exist; anything else is
a `jq` select over `list --json`.

**Trigger.** Build these *with* the report, sharing one predicate implementation. Two divergent
copies of "is this task in range" is the failure mode to avoid.

---

## 3. Structured reflections and mining them

`task add-reflection` with fields (what happened / what would have been better / proposed remedy),
stored as NDJSON, plus a `mine` query that groups by remedy and surfaces repeats.

**Why not yet.** This is the most speculative of the set, and something already does the job: a
persistent memory file accumulates the same lessons and gets re-read every session, without
taskforge in the path. Mining also needs *volume* and *repetition* to cluster anything — over a
handful of reflections it can only tell you what you already remember.

**Cheap alternative today.** `task add-comment --text "reflection: …"`. It is already there, it
already records actor and timestamp, and it is greppable. Rung two of the ladder: reuse what
exists.

**Trigger.** Grepping the `reflection:` convention has become annoying, *or* you have accumulated
enough of them that you genuinely cannot recall whether a problem is recurring. Either one means
the structure would now earn its schema.

**If built, keep the write side out.** `mine --json` should emit clusters and stop. Turning a
cluster into a durable note or a new tool is a decision for whatever consumes the output — a
reporting tool that edits your notes directory is doing two jobs.

---

## 4. Review reconciliation

An external process that reads each task's `reviews`, asks the review system whether they have
merged, and advances the task — so closure does not depend on an agent session surviving to the
end of its own work.

**This is the one with a real, observed bug behind it.** A worker opens a review and its session
ends; nothing errors, and the task simply stops moving. `--stale` and `--overdue` (already built)
make that visible, which is the cheap 80%: you can now *find* the abandoned tasks even though
nothing closes them for you.

**Why not yet.** It is the most expensive item — a second process, a schedule, and credentials
for whatever hosts the reviews — and it only pays off once there are enough tasks in flight that
finding them by hand is the annoying part.

**Design constraint if built.** taskforge must not learn what any particular review system is.
`reviews` holds opaque strings deliberately. The reconciler belongs **outside** this repo,
consuming a query like `list --awaiting-review` and driving `set-status`. That keeps this crate
free of one vendor's endpoints and auth, and means a second reconciler for a different review
system needs no change here. If two reconcilers can share one query, the boundary is right.

**Trigger.** `--stale` regularly turns up tasks whose reviews merged days ago.

---

## 5. Read-only dashboard

`taskforge serve`: a read-only HTTP view of what is in flight, what is planned, and what is
waiting on a human. Polling refresh, no controls, no mutating endpoints.

**Why not yet.** Most of it already exists. `taskforge task list` renders the board — id, status,
worker, review, expected-by — and `watch -n5 taskforge task list` adds auto-refresh for no code
at all. The genuine delta is remote viewing, and an SSH port-forward covers that.

**Cheap alternative today.**

```bash
watch -n5 taskforge task list
```

**Trigger.** A week of `watch` has told you what the real gap is — refresh rate, remote access,
or layout. Build against a known answer rather than a guessed one. Statuses already express the
sections wanted (`merged`/`stuck` = needs you, `running`/`in-review`/`changes-requested`/
`waiting-on-schedule` = in flight, `pending`/`open` = planned, terminal = excluded), so no model
change is required.

**If built:** bind loopback by default, require an explicit flag to bind any other address, and
state plainly in the README that there is no authentication. Task titles, tracker ids and review
ids are not nothing, and a read-only page still leaks them to anyone who can reach the port.

**Watch the read cost.** Listing parses every task file, so each poll re-reads the whole tree.
Fine at hundreds of tasks; measure before assuming it scales, and do not pre-optimise it.

---

## 6. Retention and archival

Reports over long ranges imply tasks are never really deleted. `archived` currently hides a task
from `list` while keeping its data, and nothing prunes anything.

**Open question rather than an enhancement:** should reports include archived tasks? Probably yes
— archiving is a "get this off my board" gesture, not "this never happened" — but that is a call
to make when the report exists, not before.

---

## Not planned

- **Interactive controls in any viewer.** Driving agents from a web page is a different product
  with a different threat model. The CLI is the interface.
- **A tracker-type enum** (`ticket_kind: jira | github | …`). `ticket` is an opaque string and one
  installation realistically uses one tracker. An enum with one inhabitant is a field that only
  costs.
- **Resolving or validating `ticket` values.** Turning an id into a title means knowing a specific
  tracker's API. Same boundary as reconciliation: it belongs in whatever wraps this tool.
