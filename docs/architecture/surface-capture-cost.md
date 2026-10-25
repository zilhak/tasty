# Immutable surface capture cost

Current persistence stores complete immutable `SurfaceData` values, including terminal history and
visible screen when content restoration is enabled. It does not store a scrollback delta or deduplicate
byte-identical captures. History compaction frees unreferenced generations; it does not make the next
live capture incremental. [Layout persistence](../features/layout-persistence/index.md) defines the
restore, undo, lazy-surface and failure contracts.

## Ownership and copies

| Stage | Owner and blocking boundary | Work proportional to content |
| --- | --- | --- |
| Coherent terminal cut | App caller holds the terminal/parser state mutex | `capture_history_and_screen` produces owned history and visible-screen rows in one lock. Memory rows are cloned; disk rows are read and decoded while that lock is held. |
| Serialization | App caller, after releasing the terminal lock | `serialize_lines` writes a complete TSSB blob. `SurfaceData::encode` serializes the small header and copies that blob into a new complete payload buffer. |
| Queue handoff | App capture preparation and journal request queue | Captured buffers move into `Work::Capture`; the handoff does not require another full-body clone. Request weights and aggregate queue ownership still apply. |
| Restore metadata lookup | Structure journal worker; ordered memory access can wait | When restore metadata exists, `RestoreLookup::fill` decodes the payload into a new body buffer and encodes the complete value again. An absent value does not trigger that rewrite. |
| Immutable write | Structure journal worker, executor/store mutex | Capture digest and payload checksum read full bodies. `put_payload_pinned` inserts a new BLOB and admission holder even for unchanged bytes; ID reservation, command and event publication remain distinct. |
| Maintenance | Same worker, only after the complete publication ACK | Models/snapshots carry content references rather than inline scrollback. Snapshot validation, retention, pin transfer, compaction and SQLite page/WAL work can nevertheless read or write payload-sized data. |

The temporary disk scrollback swap is a separate producer-side store. Rows spill there when terminal
history exceeds its memory threshold; capture reads those rows again. There is no separate journal
capture spool file between the queued buffer and SQLite. The captured buffer, terminal history,
serialization scratch and SQLite allocations can coexist; queue weights are not a hard process-RSS
limit or a count of physical copies.

The main implementations are `src/runtime/surface_capture.rs`,
`crates/tasty-terminal/src/handle.rs`, `crates/tasty-terminal/src/disk_scrollback.rs`,
`src/app/journal/capture.rs`, `src/runtime/journal_product/worker/capture.rs` and
`crates/tasty-event-store/src/payload.rs`. [Journal metrics](journal-metrics.md) distinguishes queue,
execution, publication and actual SQL COMMIT timing. Worker execution excludes subsequent publication
and maintenance; a complete capture round trip includes them. These overlapping intervals must not
be added together.

## Which sources are copied

Capture walks materialized surfaces in all local workspaces, not only the selected tab. A live
inactive terminal still contributes its full history. A `JournalPlaceholder` contributes no new
capture: its canonical data/creation-seed reference stays intact without opening a terminal or
plugin. A dormant engine's manifest and frozen aliases can retain older payloads. Maintenance may
rebase an equivalent dormant stream, but this is not a new runtime content capture.

Undo/closed-subtree records, snapshots, admission/import holders and read leases also preserve
immutable values. A reader registers its payload lease before another publication can acknowledge;
maintenance transfers those references while holding the reader-set lock. Reference lifetime is
independent of whether the terminal that originally produced the data is still visible or alive.
Missing/unreadable data keeps the existing failed-restore preservation and partial recovery behavior;
this measurement changes neither that behavior nor checksum/truncated-tail decoding rules.

A full capture must still pass the existing per-request and aggregate queue bounds. The history
lower-bound gate can reject an impossible capture before reading disk rows. Passing that lower bound
is not admission: the complete encoded request is checked too. This measurement does not reduce
history, change the 64 MiB limit, or use the user-close path's separate omission policy to reduce cost.

## Reproducible production-path measurement

The Linux-only ignored unit test
`runtime::journal_product::tests::capture_cost::measure_full_immutable_capture_between_maintenance_cuts`
is an explicit measurement/control, not a latency threshold. It uses detached terminals (no PTY,
shell, GUI, network or stored-command execution), real `SurfaceData`/TSSB codecs, the production
`JournalWorker` request/publication/ACK path and post-ACK maintenance. A synthetic legacy fixture
initializes canonical models and a checked dormant restore manifest; unrelated JSON serialization is
not used as a replacement for terminal capture.

Each of four cases runs five independent pairs of maintenance cuts: 1,000 rows unchanged and with
one row appended, and 50,000 rows unchanged and with one row appended. Rows contain deterministic
Unicode cells and wrapped flags with default attributes, plus eight nonempty visible screen rows. The memory threshold is 512 rows, so both
sizes exercise real disk-backed history. Every request fits the unchanged queue bound, and only one
request is in flight. The worker fills empty metadata to exercise its real full-payload rewrite;
no executable command is stored.

Use a private temporary root and serialize this run with other build/native measurements:

```sh
capture_root=$(mktemp -d)
mkdir -m 700 "$capture_root/data" "$capture_root/tmp"
env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS \
  TASTY_HOME="$capture_root/data" TMPDIR="$capture_root/tmp" \
  TASTY_CAPTURE_COST_OUTPUT="$capture_root/measurements.json" \
  cargo test -p tasty --lib --locked \
  runtime::journal_product::tests::capture_cost::measure_full_immutable_capture_between_maintenance_cuts \
  -- --ignored --exact --nocapture --test-threads=1
```

The metadata-only JSON records full-path capture time, worker round trip, cached worker execution,
queue/completion/COMMIT counters, newly retained BLOB bytes, swap-file sizes at both cuts, append/spill work and Linux per-thread I/O.
Separate phase probes time history cloning, full TSSB serialization, payload decode/copy, encode/copy
and metadata rewrite on the same deterministic cut. They are separate repeated operations, not
summands of one transaction. Clone byte fields describe initialized text/cell-length/default-attribute
and row storage, not allocator capacity, refcount traffic or a measured `memcpy` total.

Caller I/O counters include telemetry overhead; the adjacent telemetry-only control records it.
Worker I/O counters are read from another thread. `rchar`/`wchar` are syscall bytes, not storage-device
traffic. `read_bytes`/`write_bytes`, SQLite BLOB lengths and DB/WAL file sizes have different meanings.
Warm page-cache reads can report zero device bytes while copying many megabytes. Record the build
profile, source/binary hashes, filesystem, raw repetitions and competing load before interpreting
latency. Do not extrapolate this warm debug measurement to release, cold storage or another OS.

Controls verify the entire published body digest and exact TSSB byte round trip, complete row count,
unchanged-body equality, append-prefix preservation, fresh payload generations, coherent maintenance
cuts, lazy capture skipping and dormant content/View preservation. An old payload remains readable
with an active read lease across the next cut; this is not an isolated proof that the lease is its
only remaining holder. Existing retention/undo/recovery tests remain separate obligations.

No delta/chunk implementation or performance improvement is established by this benchmark. A future
storage representation would need independent lossless, lifetime, recovery and old/new performance
validation; serialized-size arithmetic alone does not establish a latency benefit.
