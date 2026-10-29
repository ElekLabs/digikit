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
