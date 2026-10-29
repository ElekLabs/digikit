# Reusable SDK file catalogue

2026-10-30. DT2 OS 1.17 and DN2 OS 1.12. **[D]** inspected or tested here;
**[O]** integration still open. Supports the [sampler plan](../plans/dsp-sdk-and-sampler.md).

## Shared service direction

Em requires listing to be reusable SDK functionality for a later file manager.
Directory traversal and metadata belong in that service; sample-format filtering
and track binding belong in the sampler. The public C contract is not implemented
yet. Preserve full paths, type, size and provider identity, with explicit page,
completion, failure and stale-result states. Display labels are not identities.
DT2's hash and DN2's SHA must not be treated as equivalent content guarantees.

## DN2 store cursor

**[D]** `modules/core/runtime/sdk/src/store/catalog.rs` adds an allocation-free
`CatalogCursor` and owned `CatalogEntry` values. Open captures both header
fingerprints around existing verified table selection, preserving redundant-copy
fallback. It retains only I/O, layout, selected header, fingerprint, next raw
index and sticky error. Full-table CRC and tail checks occur once at open.

Each nonempty page reads at most one table chunk, bounded by caller scratch,
output capacity and 128 sectors (64 KiB), plus four one-sector header reads.
Odd entry boundaries may reread the shared sector. No prefix counting or sample
filtering can turn one page into a full-table scan. EOF still checks generation.
Changed headers, I/O errors, invalid buffers, unterminated paths and unknown entry
kinds poison the cursor. Callers discard all rows from that cursor after failure.
Returned paths retain all 159 permitted bytes plus NUL; metadata includes kind,
size, first sector and SHA. Metadata enumeration does not validate file contents.

**[D]** Six new tests cover pagination/metadata, maximum paths, bounded I/O, empty
tables and EOF freshness, preferred-copy corruption fallback, mutation timing,
I/O failure, bad buffers and malformed records. The SDK host suite, no_std build,
named owner pytest and independent diff review pass. Logs are
`out/catalog-sdk-tests.log`, `out/catalog-sdk-owner.log` and
`out/catalog-sdk-nostd.log`; the owner pytest recompiles the final six-test diff.
Scoped document path checks and diff whitespace checks pass. Routed
`mise run test:changed` passes layering but still fails the existing ten stale
references in five unchanged files (`out/catalog-changed.log`). No merge gate or
device execution is claimed for this backend-only checkpoint.

**[O]** Initial verification is table-size dependent; schedule it outside UI
callbacks. A fingerprint is observed-change detection, not a lease or ABA/collision
guarantee. The future directory adapter still needs bounded filtering/progress and
implicit-folder handling. No public API, UI wiring or sample publication changed.

## DT2 firmware iterator ABI

**[D]** Static discovery plus an independent original-byte audit identify a plain
data seam beneath the stock `FileSystemDirectory` wrapper:

- `0x40157338(directory_id, state)` initializes caller-owned 16-byte state:
  ID at +0, page at +4, byte offset at +8, indexed ordinal at +12. The initializer
  writes the ID and clears the remaining words. The decompiler omitted the ID
  store; original instruction `0x4015733c` establishes it.
- `0x4015734e(state, output)` advances the iterator. A private zeroed `0x114`-byte
  output matches the stock caller. Success returns 1; exhaustion returns 0;
  invalid readiness/pointers return -1. Inspect output only on success.
- Output fields are ID +0, uint16 type +4, directory ID +8, page/offset +12/+16,
  and a 256-byte name at +0x14. Helper `0x40155fd6` reads an unsigned byte name
  length at `0x401560e2`, copies the bytes, then writes the terminator.
- `0x40136388` is a leaf memcpy. The stock caller `0x40159d08` allocates and
  zeroes this buffer and wraps it in shared ownership for its collection. The
  entry itself needs no C++ string constructor or destructor. An initial inference
  that the name was a C++ string was rejected by the byte audit.

**[D]** Iterator mutex `0x46f4deb0` is held across the lower reader. The `0xfff`
metadata threshold selects indexed versus sequential lookup; it is not an
iteration limit. Sequential traversal can skip records and cross 32 KiB pages.
Cache misses can read 32 KiB; lock contention can reschedule. No fixed per-call
latency or immutable directory snapshot has been established.

Audit input: `out/sections/dt2-1.17/section_3_MAIN_OS.bin`, SHA-256
`a1e7b657b705eba1a19d81c33c1e11ba9c409816447ad74005d7bbf36da6d964`.
Queries use `tools/coldfire/fw.py` on `dt2-1.17`: dossiers for `0x4015734e`,
`0x40159d08`, `0x40157338`, `0x40155fd6` and `0x40136388`; raw initializer and
name-copy instructions were independently decoded from the original image.

**[O]** Next proof: acquire a known-valid directory ID after filesystem readiness,
call the iterator in task context with guarded private state/output, compare rows
against stock ReadDir, and exercise end/error cases. Path-to-directory resolution,
type decoding, per-entry size/hash lookup, concurrent mutations and storage-error
handling still need adapter evidence. No firmware declaration or on-device execution
is claimed by this static audit.

## Path opening and stock metadata follow-up

**[D]** An independent original-byte audit resolves a plain path opener:
`0x401572a0(const char *path, u32 state[4])`, returning 0 on success and -1 on
failure. It checks filesystem readiness, null arguments and path length at most
255, holds the filesystem mutex while traversing components, accepts a directory
result only, and initializes all four state words. Root `/` resolves to ID 2.
Missing paths and file-as-directory inputs are rejected. It uses shared component
scratch internally; callers must not add an outer filesystem lock.

**[D]** Existing declared `cf.storage.fn.path.size` resolves to `0x4015bcfe(id)`
and reads node +4 under its own lock, returning `0xffffffff` on failure. Stock
ReadDir's producer instead uses locked metadata getter
`0x4015aec2(id, u32 out[4])`: 0 succeeds with `{id, node[+0xc], node[+4], node[+0x10]}`;
-1 means unavailable/invalid data. The getter requires the low bit of node +0xc.
The RPC producer passes that stored odd hash unchanged; it does not recompute it.
RPC size is `max(raw_size - 16, 0)`. Getter failure falls back to zero hash/size.
This does not prove every directory necessarily takes that fallback.

**[D]** Dirent type 1 maps to `D`, all other values to `F` in the stock RPC.
The final row loop filters exact `.` and `..`, with no general hidden-name filter.
The preceding `FileSystemDirectory` wrapper consumes two leading raw records.
Their names must be observed in a live listing; position alone does not establish
that they are dot records. The diagnostic retains all raw rows before comparison.

**[D]** `core/file-catalog-proof` adds DT2-only hashed anchors for directory open
and next and a private guarded OPEN/NEXT/SIZE mailbox. It invokes at most one
firmware call per request from the actual UI-loop hook, preserving the original
queue-pop ABI. It conflicts with the runtime and changes no DSP code/counts.
The opt-in `tests/regression/runtime/file-catalog.test.ts` uses explicit prebuilt
binaries/card, stock RPC fixtures, bounded waits and owned-daemon cleanup.
CF object build/check, generation, layering, TypeScript checks and independent
review pass. This supersedes the earlier declaration gap; native execution,
concurrent mutation and storage-error robustness remain separate checks.

## Live DT2 directory proof

**[D]** The named opt-in Vitest case passes on DT2 1.17, with a second agent
checking the saved plan, image hash, raw buffers and observations independently.
Run: `out/runs/file-catalog/808e86fa-814f-4b46-92a0-660fc9c20162`;
log: `out/file-catalog-live.log`. The patch contains only `core/expansion` and
`core/file-catalog-proof`; section 3 changes, DSP sections remain stock. Patched
SYX SHA-256: `d5b1642d4a65809130fc4dbbe9a10931bed160e9cced5288178f29807312b2f3`.

The explicit prebuilt card is a generated three-sample fixture with the current
firmware hash. It is opened read-only and writes stay in the owned daemon's
overlay. Stock RPC creates `/catalog-proof/item.bin` with 257 deterministic bytes.

- Raw root records start with `.` and `..`, then `catalog-proof`, `factory`,
  `1-sine220`, `2-sine880`, `3-noise`. The child starts with `.` and `..`, then
  `item.bin`. Removing only the dot entries matches stock names and types.
- Raw directories have type 1 and the observed files type 0. Sample sizes are
  96,080 physical bytes versus 96,064 in ReadDir; the child is 273 versus 257.
- The 27 mailbox operations invoke 20 firmware calls, at most one per request.
  Repeated EOF performs no extra firmware call. Unopened NEXT, an unterminated
  path, missing path and file-as-directory are rejected. Failed OPEN invalidates
  the iterator, and subsequent NEXT invokes no firmware call.
- All four canaries remain unchanged throughout and in the final buffers.
  CF reports DT2 1.17 ready, filesystem verified, no error, no heartbeat stall
  and zero guard hits.

The prebuilt daemon reports its cached DSP profile as DN2 1.11 while running the
current DT2 image with instruction guards/fallback. This is a ColdFire directory
ABI proof, not an audio, hardware or timing claim; no generated-core change was
needed. The locked metadata/hash getter has static byte evidence only, not a
call in this diagnostic. Concurrent mutations and storage failures remain open.
The generic public SDK service, DN2 implied-folder projection and picker remain
implementation work.

Checkpoint checks: the named live test passes, as do 195 direct host tests
(16 skipped), scoped TypeScript types/lint/format, object/generation checks,
layering and changed-document path checks. Routed `test:changed` still fails
the ten pre-existing stale references and the workspace pnpm launcher before
its host/lint/format/types commands run. Direct checks cover those code checks;
logs are `out/file-catalog-{live,host,changed,types,lint,format}.log`.
No merge gate or hardware run is claimed.

## Bounded directory pages and incremental opening

**[D]** `store/directory.rs` adds `DirectoryCollector<N>`: full-path byte ordering,
exclusive continuation, deduplicated immediate children (including implied and
empty folders), exact file metadata and no heap/I/O. It exposes no page until the
caller finishes the complete raw scan with a valid generation. Memory holds N
entries; each directory page replays the table with O(table entries * N) insertion
work. Malformed paths/kinds and requested-file/ancestor-file cases fail closed;
conflicting metadata checks cover retained children, not the whole table.

**[D]** `CatalogOpen` advances verified opening without a full-table loop per
scheduler step. Initial capture uses at most six header-sector reads. Later steps
use four header reads plus one table read bounded by the caller's scratch and
128 sectors; incremental CRC and tail checks share the chunk. Preferred-table
CRC, tail or data-read failure falls back to the other captured header, including
identical header copies. Captured-header read failures are intentionally strict
`E_READ`; the legacy opener could hide a transient failed B read. The existing
synchronous opener remains unchanged. `take` refuses incomplete verification;
`rewind` checks the retained fingerprint and resets iteration without a full
table revalidation. Neither operation establishes an immutable lease.

**[D]** Eight projection tests plus seven incremental-open/rewind tests bring the
SDK host suite to 80 passing tests. no_std compilation, the named owner pytest,
scoped formatting and independent review pass. Logs:
`out/directory-sdk-{tests,nostd,owner}.log` and
`out/catalog-open-{tests,nostd,owner}.log`. No new native timing claim follows.
The [service design](../design/sdk-files.md) records client ownership, request
generations, copied result lifetime and notification/rollback invariants before
the public ABI is exposed.

**[D]** `files_job.rs` adds a fixed-client, allocation-free lifecycle broker.
Owned requests and rows, separate work/event leases, nonreused positive IDs and
per-lease nonces prevent late or duplicated completions from altering a newer
request. Cancel/retire invalidate queued work and notifications immediately;
active work/callback pins remain until their exact lease returns. A retired client
cannot be reused before both pins drain. Poll returns a copied row. Readiness
hints coalesce and an old callback's return cannot clear a newer edge.
Thirteen interleaving/exhaustion tests bring the SDK suite to 93 passing tests;
no_std, the owner pytest and independent review pass (`out/files-job-*.log`).
External synchronization, actual callback lifetime and loader retirement are
runtime integration obligations, not established by this pure model alone.

**[D]** A further original-byte audit finds that stock DT2 ReadDir's flag tests
node byte +1 bits 0 and 2 (`0x4015be58` / `0x4015bea2`). Its interpretation as
read-only/protected/factory remains **[O]**, and failed getter paths can normalize
to true. Do not use it as an authorization flag. The locked metadata getter
`0x4015aec2` copies all four words on success and leaves output untouched on
failure; it retains no caller output pointer. Its declaration candidate is
`cf_func` hash `56566ccf1985a1fe2868df8f8d909661`, offset 0, length 4. It has not
been added to the runtime or executed by the private directory proof yet.

This SDK checkpoint also passes scoped document paths and diff checks. Routed
`test:changed` passes layering and retains only the known ten stale references
in five unchanged files (`out/files-sdk-changed.log`). No runtime integration,
new device proof or merge gate is claimed for these pure helpers.
