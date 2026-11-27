# The +Drive format

All addresses are **DT2 1.16** (`out/ghidra/dt2-1.16-emac/`, file offset =
`VA - 0x40000400`), read from the static Ghidra decompile plus the raw bytes
of `Digitakt_II_OS1.16.syx`'s extracted sections
(`sections/.source-sha256` = `278541e4...`). Nothing here required the
emulator; two independent static passes (one per topic) plus a third pass on
the filesystem layer were run, and a few claims were cross-checked against
raw bytes captured from a real firmware-formatted card overlay
(`snapshots/dt2-1.16/running.snap`), which counts as image-byte
cross-checking for those specific claims. A fourth, independent agent then
re-read the filesystem layer's 8 most load-bearing claims cold (record
layout, extent format, root id, the readdir/`0x10001` mechanism, the raw
write path) against the same decompile before seeing this document, per this
repo's rule that a finding needs a second agent's check against the image
bytes before it's marked **[V]**; one claim (the readdir/`0x10001`
mechanism) came back **[C]**, corrected in place below. Everything else is a
single static pass and marked **[D]**, not **[V]**.

`docs/findings/02-machines-and-parameters.md` (~line 328-340) and
`docs/findings/07-emulator.md` (~line 1345) cite `FUN_401239be`,
CMD18 = `0x401208fe`, CMD25 = `0x40120ae4`. **[C]**: none of these addresses
exist in the 1.16 dump (`0x401208fe` decompiles as an unrelated SysEx
receive/ack state machine). The real 1.16 functions, found by matching
behaviour and by the ESDHC `XFERTYP` command-index literal, are:

- `FUN_40130f9a` (0x40130f9a) — the "INITIALIZING +DRIVE..." driver: sets the
  header's fixed bytes and calls the two format gates.
- `FUN_4012deda` (0x4012deda) — CMD18 read (`XFERTYP = 0x123a0036`).
- `FUN_4012e0c0` (0x4012e0c0) — CMD25 write (`XFERTYP = 0x193a0022`).

Both arm eDMA channel `0x3b` = 59, matching `docs/findings/04`'s existing
"eDMA channel 59 is eSDHC block I/O" line — that citation used the same stale
address and should be read as `FUN_4012deda`/`FUN_4012e0c0` going forward.

## Two separate on-disk regions, not one

+Drive is **not** a single filesystem. There are two independent structures,
addressed by disjoint, hard-coded absolute sector numbers (not derived from
card capacity or a superblock pointer):

1. **`MmcFs`'s three fixed-capacity object pools** — projects, sounds
   (patches/presets), kits. Flat, slot-indexed, no directories. This is what
   the first-boot "Factory reset" worker (`FUN_400334bc`) formats.
2. **A real, arbitrarily-nested, path-addressable filesystem** — inode-style
   128-byte records, extent-mapped 32 KB content pages, hierarchical
   directories — reached through a `Directory`/`FileSystemDirectory` class
   pair and a generic `elektron::MidiRpcFsSample*` MIDI SysEx RPC family used
   by companion software (e.g. Transfer) to browse/read/write it remotely.
   **This is where +Drive sample files live**, and it's the one this finding
   and `tools/firmware/plusdrive.py` targets.

Nothing found ties the two regions together at the code level (a `MmcFs`
vtable scan for the filesystem's sector constants came up empty), only by
their sector numbers being far apart. **[D][O]**: not exhaustively proven
disjoint, only not found joined.

## Region 1: `MmcFs`'s header, table, and pools **[D]**, header partly **[V]**

### Header sector (block 0)

Real freshly-formatted bytes, captured from `running.snap`'s `Esdhc` card
overlay (`emu/snapshot.py`'s pickled `components['esdhc']['card_overlay']`,
a sparse `{byte_offset: value}` dict — no Machine construction needed to read
it):

```
offset 0x000: be ef ba ce 00 00 00 01 01 01 00 00 00 00 00 00
offset 0x1c0: 00 00 00 00 00 00 00 02 43 17 2d e4 00 00 00 00
offset 0x1d0: 00 00 00 00 45 05 5b c0 45 05 5b d0 45 05 5b d0
(everything else in the 512-byte sector is zero)
```

`FUN_40130f9a` (0x40130f9a), quoted:

```c
param_1[0x779c] = -0x41104532;      // ctx+0x1de70 (sector +0x00): 0xBEEFBACE
param_1[0x779d] = 1;                // ctx+0x1de74 (sector +0x04): 1
*(undefined1 *)(param_1 + 0x779e) = 1;        // ctx+0x1de78 (sector +0x08): 1
*(undefined1 *)((int)param_1 + 0x1de79) = 1;  // ctx+0x1de79 (sector +0x09): 1
cVar4 = FUN_4012fd92(param_1);
```

`FUN_4012fd92` (0x4012fd92), the writer, requests **exactly 12 bytes** be
written to block 0 (`FUN_4012fcb6(param_1,0,0xc,param_1+0x1de70,...)`), but
the CMD25 primitive `FUN_4012e0c0` always rounds the DMA up to a whole
512-byte sector (`uVar2 = (param_2+0x1ff)>>9`, forced to 1). **So bytes
12-511 of the transferred sector are whatever RAM happened to be at
`ctx+0x1de70+12..+0x200`, not code-written data.** `ctx+0x1e034` is used
throughout this object's methods as an RTOS mutex lock handle
(`FUN_4019e0ee`/`FUN_4019e114`), and `0x1e034 - 0x1de70 = 0x1c4` — inside the
observed nonzero region (0x1c0-0x1e3). The three near-identical 32-bit words
at 0x1d4/0x1d8/0x1dc (0x1d8 and 0x1dc *identical*) have exactly the shape of
an empty RTOS wait-queue's `next==prev` sentinel. **[V]**: the mechanism (a
12-byte intentional write, rounded up to 512, leaking live mutex/queue state
into the tail) is proven by the code; the exact semantics of those leaked
words are not, and don't need to be — they are not part of the format.

**Header field table:**

| offset | bytes | meaning | confidence |
|---|---|---|---|
| 0x00-0x03 | `BE EF BA CE` | magic | **[V]** literal write |
| 0x04-0x07 | `00 00 00 01` | format/version = 1 | **[V]** literal write |
| 0x08 | `01` | flag | **[V]** literal write |
| 0x09 | `01` | flag | **[V]** literal write |
| 0x0A-0x1FF | zero in the sample | not written by this function; leaked live RAM in general | **[D]**, not a format field |

`0xBEEFBACE` is a generic Elektron container magic reused elsewhere (e.g.
`FUN_4000b8c0` checks it as a sound-bank/preset blob magic; `FUN_4012f6d8`
reuses the literal as an internal end-marker) — not unique to +Drive.

### Table sector (block 0x800 = byte offset 0x100000)

Real captured bytes: **all zero**, matching a freshly formatted (nothing
allocated) card, not a missing write. `FUN_4013009c` (0x4013009c) writes
**400 bytes** from `ctx+0x1de7d` to block `0x800`. `FUN_40130f9a` zero-fills
three sub-ranges of that same buffer before the first format:

```c
FUN_40135ad8((int)param_1 + 0x1de7d, 0x10);   // 16 bytes  = 128 bits  -> PROJECT pool
FUN_40135ad8((int)param_1 + 0x1de8d, 0x100);  // 256 bytes = 2048 bits -> SOUND pool
FUN_40135ad8((int)param_1 + 0x1df8d, 0x80);   // 128 bytes = 1024 bits -> KIT pool
```

16+256+128 = 400 bytes exactly. **[V]**: block 0x800 is three fixed-size
per-pool occupancy bitmaps (128 project slots / 2048 sound-preset slots /
1024 kit slots), not a directory, inode table, or disk-wide free-space map.
Confirmed bit setters: `FUN_40130234` (project, base `ctx+0x1de7d`),
`FUN_40132b8a` (kit, base `ctx+0x1df8d`, clamped `<= 0x3ff`); the sound-pool
setter wasn't traced line-by-line but the 0x100-byte zero-fill and a
2048-iteration rescan (`FUN_4012fa8c`) match exactly.

### Pools have no growth mechanism and no directories **[V]**

Slot→block address is a pure clamped linear formula, not a chain:

```
FUN_4012eef2 (project):  slot>0x7f -> 0x7f;   block = ((slot+0xb)&0xff)<<0xf
FUN_4012f596 (sound):    slot>0x7ff -> 0x7ff; block = (slot+0x2400)*8
FUN_401324cc (kit):      slot>0x3ff -> 0x3ff; block = (slot*0x10000+0x2c00000)>>9
```

Each sound-pool slot's own on-disk header (`FUN_4012f6d8`, reads 0x600 bytes
per slot) carries a 4-byte magic (`0x44543153`="DT1S" Digitakt /
`0x444e3153`="DN1S" Digitone — a shared cross-device format), a 16-byte name
field, and two more 4-byte fields (size, format code). "FACTORY
PROJECT/SOUNDS/KITS >> +DRIVE..." (`FUN_40131282`, strings at 0x4025a6df /
0x4025a6fc / 0x4025a718) are the three progress captions for formatting each
pool in turn, **not** evidence of a folder hierarchy — at this layer storage
is flat and fixed-capacity. This corrects any folder-hierarchy reading of
those strings in `docs/findings/02`/`03`. No literal "2,120 erase groups"
(`docs/findings/07`) was found compiled in along this path; it's presumably
read from the card's own CSD/EXT_CSD at erase time — **[O]**, not chased.

## Region 2: the real filesystem — what `tools/firmware/plusdrive.py` writes **[D]**

Two record kinds:

- **A 128-byte per-file/per-directory metadata record** ("inode"), reached
  only through `FUN_4015b64e(id)`.
- **A variable-length directory-entry record**, embedded in a *directory's
  own content stream*, one per child.

### Disk layout (absolute sector numbers, hard-coded in the firmware)

| region | sector base | source |
|---|---|---|
| record-id occupancy bitmap | `0x5d8040` | `FUN_4015b7ac` |
| content-page occupancy bitmap | `0x5d80c0` .. `0x5d8180` | `FUN_40155698` (flush loop ends exactly at 0x5d8180 — cross-check) |
| record array (128 B/record) | `0x5d8180` | `FUN_4015b64e`: `id*0x40 + 0x5d8180` groups 256 records/32 KB page |
| main content-page area (32 KB/page) | `0x5ee180` | `FUN_401559f4`: `page*0x40 + 0x5ee180` |
| second, smaller content region (purpose unresolved) | `0x458000` | `FUN_401559f4`, for page ids `>= 0x1000800` |

These numbers are far below the pools' ~214 MB footprint on one side and
well inside the default 0x00760000-sector (≈3.69 GiB) card capacity
`emu/esdhc.py` already uses, so both regions fit in the emulator's existing
default card size unmodified.

**Root directory has a fixed, literal id: 2.** `FUN_40159f50` (the path
walker): `if (*param_1 == '/') { local_128 = 2; ... }`. The id-2 record sits
at absolute byte offset `0x5d8180*512 + 2*128`. There is no separate
superblock region — "the superblock" is just "id 2, resolved through the
ordinary record path". **[V]**: also confirmed the literal `"/\0..\0\0"` path
string at raw file offset `0x25d545` (`0x4025d945 - 0x40000400`) directly
against section bytes, not just the decompiler.

Nesting is real, not flat: `FUN_40159f50` walks arbitrary `/`-separated path
depth, calling `FUN_401572e0` (name→child lookup) and descending into
whichever child it returns, recursively. **[V]**.

### The 128-byte record ("inode")

```c
int FUN_4015b64e(uint param_1) {
  if (param_1 - 2 < 0x57ffe) {          // valid on-disk ids: 2 .. 0x57fff
    ... /* 16-slot LRU cache, then on miss: */
    FUN_4012deda(((param_1 & 0x1ffffff) >> 8) * 0x40 + 0x5d8180, 0x8000, 0x40941d90);
    FUN_401360ac(iVar3, (param_1 & 0xff) * 0x80 + 0x40941d90, 0x80);
    return iVar3;
  }
  else if (param_1 - 0x1000000 < 0x800)
    return (param_1 - 0x1000000) * 0x80 + 0x47d73ed0;   // RAM-resident ids, purpose unresolved
  return 0;
}
```

Field offsets, each confirmed by a one-line accessor:

| offset | size | meaning | confidence |
|---|---|---|---|
| 0x00 | 1 (bit 0) | attribute bit, tested by `FileSystemDirectory::vfunc_7` (`FUN_4015bb34`) | **[D]**, semantics (file-vs-dir?) unresolved |
| 0x01 | 1 (bit 0) | attribute bit, tested by `vfunc_9` (`FUN_4015bb7c`); fresh records default to `2` (bit0=0); a parent must have this bit **clear** before a child can be created under it (`FUN_40158626`) | **[D]**, "is-directory"/"protected" both plausible, not disambiguated |
| 0x02 | 2 (u16) | a counter incremented at link time (`FUN_40156334`) | **[O]** link count vs open-file count unresolved |
| 0x04 | 4 (u32) | content byte size | **[V]**, independently read in 3 places (`FUN_4015ba22`, `FUN_40155cfa`, `FUN_4015838e`/`File::write`'s grow-on-write check) — a second agent confirmed these three but found a fourth function that looks like a size-getter, `FUN_4015ba7c`, does **not** read this field: it instead sums `extentLength * 0x8000` over the extent list, i.e. *allocated* bytes, a different quantity. Don't cite `FUN_4015ba7c` as a size-field reader. |
| 0x08 | 4 (u32) | parent directory's own record id, written at link time | **[V]** |
| 0x0c | 4 (u32, bit 0) | attribute bit, tested by `vfunc_14` (`FUN_4015bc10`) | **[O]** unresolved |
| 0x10 | 4 (u32) | monotonic global sequence number, set only at allocation | **[V]** field exists; purpose inferred (generation/tiebreak counter) |
| 0x1e | 2 (u16) | extent count | **[V]** |
| 0x20-0x7f | 96 B | up to 8 inline 12-byte extents (count <= 8), or up to 24 4-byte pointers to indirect 32 KB extent-index pages (count >= 9, 2730 extents/page) | **[V]**, arithmetic cross-check: `8*12=96=0x60`, `0x20+0x60=0x80` = record end; `32768/12` truncates to 2730 = the exact divisor used |

An extent is a 12-byte triple `{logicalStartPage: u32, lengthInPages: u32,
physicalBasePageId: u32}` (proven by the lookup function's bounds check and
mapping arithmetic), not a `(start,length)` pair.

**Reserved logical pages, addressed the same way as real content pages via
extents in the directory's own record:** `0x10000` (name-hash index, binary
search, used by `FUN_401572e0` when doing a by-name lookup with more than
4094 real entries per the report that found it — direction not fully
resolved), `0x10001` (the enumeration/"jump to entry N" index that
`FUN_40157072`, the readdir primitive `FileSystemDirectory` uses to build
its on-screen list, reads **unconditionally**), `0x10002` (id-sorted index).
`FUN_40156334` (the link/create function) maintains all three on every
insert.

`FUN_40157072`, quoted (this is what the SRC/sample browser's enumeration
ultimately calls, through `FileSystemDirectory`'s `std::_Sp_counted_ptr<
dirent2_t*,...>::ctor_dtor` at 0x40159a2c):

```c
undefined4 FUN_40157072(undefined4 *param_1,int param_2) {
  ...
  puVar3 = (ushort *)FUN_40155c16(*param_1,0x10001);
  if (*puVar3 < 0xfff) {
    iVar1 = param_1[3];                      // current entry index
    if ((int)(uint)*puVar3 <= iVar1) { ...; return 0; }   // >= count -> end of directory
    uVar2 = *(uint *)(puVar3 + iVar1 * 4 + 6);            // packed (page,offset) for entry iVar1
    param_1[3] = iVar1 + 1;
    param_1[2] = uVar2 & 0x7fff;                          // byte offset within page
    param_1[1] = uVar2 >> 0xf | (int)-((int)uVar2 < 0) << 0x11;  // page index, sign-extended
  }
  ...
  uVar4 = FUN_40155cfa(param_1,param_2);      // decode the entry at (page,offset)
  ...
}
```

**This is the load-bearing detail for our writer, and a second-agent check
of this specific claim found the mechanism I first wrote here was wrong —
correcting it: [C].** `FUN_40155c16(dirId, 0x10001)` resolves through
`FUN_40157e60`'s extent lookup; when no extent in the directory's record
covers logical page `0x10001` (never allocated), that lookup's not-found
path returns 0, so `FUN_40155c16` returns **NULL**, not a pointer to a
zero-filled page. `FUN_40157072` then does `*puVar3` with **no NULL check
anywhere in the function** — this is an unguarded NULL dereference, not a
clean "reads as zero, reports empty" fallback. What actually happens depends
on runtime content at address 0 (on this ColdFire target, plausibly the
exception-vector table's initial supervisor-stack-pointer value, which would
almost certainly not read as `< 0xfff`, so even the branch this paragraph
originally described wouldn't be taken — the function would instead fall
through to decode whatever stale cursor state it already had). The
conclusion for the tool is unchanged and, if anything, stronger: **a
directory missing its `0x10001` index page is not a graceful "shows up
empty" case, it's undefined behaviour / a likely crash**, so
`tools/firmware/plusdrive.py` must always write this page for the root directory.

Page `0x10001` layout, inferred from the read pattern (u16 count at offset
0, then a 4-byte gap, then one 4-byte packed pointer per entry starting at
offset 6): `count:u16` at 0, 4 reserved bytes, then `count` x 4-byte packed
`(page:17bit signed, offset:15bit)` pointers into the directory's real
content pages. **[D]**, not independently cross-checked against a real
populated directory (none was available to capture).

### Superblock state and checked words decide the boot-time filesystem check **[D]**

Read statically on DT2 1.17 (`FUN_4015a72c` is the mount, `FUN_4015a876` and `FUN_4015a928` write back). The superblock is sector `0x5D8000` (an eMMC sector number for CMD18/CMD25, so byte offset `0x5D8000 * 512` in the card image, not NOR flash), read to RAM `0x46f4dcb0`. The checksum is at `0x1fc`, the `hashlittle` of bytes `[0, 0x1fc)` with seed `0x31323334`. State is at `+0x04` and checked at `+0x34`. Mount accepts state 3 or 4. `MainScreenView::vfunc_5` runs the check ("VERIFYING FILE SYSTEM...", then "FILE SYSTEM OK") when the drive is mounted and state is 3 or checked is 0 (`FUN_4001058c`). After a good check the firmware writes state 4 and checked 1 and recomputes the checksum. `build_superblock` (`packages/emulator/plusdrive/src/format.rs`) and `tools/firmware/plusdrive.py` wrote checked 0, so every card they built ran the check at its first boot; they now write checked 1.

> **Corrected 2026-09-28 [C][V].** Wrong. All three index pages (`0x10000`,
> `0x10001`, `0x10002`) hold 8-byte entries from byte 8, `{u32 key, u32
> page * 0x8000 + offset}`; the readers address them as `(ushort *)p + i*4 +
> 6`. The old one-file image worked by accident (the pointer read as 0 =
> offset 0). A directory also needs "." and ".." as its first two entries
> and all three index pages. See "Native sample files and a boot project"
> at the end of this file.

### Directory content-page entry format (variable length)

Confirmed from both the reader (`FUN_40155cfa`) and the writer
(`FUN_40156334`):

| entry offset | size | meaning |
|---|---|---|
| 0x0 | 4 | child's own record id (0 = empty/free slot) |
| 0x4 | 2 (u16) | total slot length in bytes (stride to next entry) |
| 0x6 | 1 | name length in bytes |
| 0x7 | 1 | type/kind byte — a copy of the child record's own offset-0x00 byte, cached here so listing doesn't need a second `FUN_4015b64e` fetch |
| 0x8.. | nameLen | raw name bytes, not NUL-terminated on disk |

**[V]**.

### Write path does no interpretation of the payload **[V]**

`File`'s write vfunc (`FUN_4015838e`) allocates/grows pages
(`FUN_4015816c`) then either writes a whole 32 KB page straight from the
caller's buffer (`FUN_40155ba8`) or `memcpy`s a partial page
(`FUN_401360ac`). **No chunk-tag check, no header validation, no
format-specific branching on the bytes being written, anywhere in this
path.** Whatever bytes a caller (the MIDI RPC handler, or our own tool)
supplies become the file's bytes, unmodified.

Separately: `docs/findings/04-coldfire-dsp-link.md` (line 1372) already
established the *unrelated* fact that a debug-console command handler
(`FUN_400cae8c`) can resample raw PCM through the ColdFire's EMAC against a
fixed SDRAM window, but that path is **only reachable from the debug
console**, not the real track engine.

### On-disk sample payload format: **[O]**, genuinely open

No `RIFF`/`WAVE`/`fmt `/`data` chunk-tag string exists anywhere in any
extracted 1.16 section (`section_2_DSP.bin` through `section_8_SECTION.bin`)
— checked twice, once by raw byte scan and once against Ghidra's own
`strings` table; the few `RIFF`/`WAVE` substring hits are all inside
unrelated words (`SHERIFF`, `WAVELENGTH`, ...). No function under a
`Sample*`/`Wav*`/`Pcm*` name does bit-depth/channel/samplerate range
checking. The only registered file extensions anywhere in MAIN_OS are
`.dt2prj`/`.dt2kit`/`.dt2pst` (project/kit/preset, not samples), registered
alongside the literal string `"48000"` (the only bare samplerate string in
the image — inferred to be the device's native/internal samplerate, not
proof of anything about sample-file encoding).

Given the write path is a raw, unvalidated byte copy (previous section),
either (a) the SRC browser/loader validates and converts format at *load*
time (in code not identified by this pass — `SampleManager::vfunc_30`
through `vfunc_45`, 0x40023e84-0x4002c794, and `SampleLoaderBgWorker`'s own
read path, 0x400f0d30/0x401cbb1c/0x401cbb42, are the untraced candidates),
or (b) any WAV→internal conversion happens entirely on companion desktop
software before the bytes ever reach this filesystem, and firmware always
treats sample-file content as an opaque blob. **`tools/firmware/plusdrive.py` stores
each input WAV file's bytes verbatim (whole file, including its RIFF
header)** — the only choice consistent with the proven write-path behaviour,
and the one to falsify or confirm with an emulator run against a real SRC
browser/loader trace.

> **Corrected 2026-09-28 [C][V].** Answered: the loader reads a native
> format (64-byte header, big-endian int16 PCM, 16-byte trailer), written
> by the sampler's own "Save recording" path. A WAV stored verbatim fails
> the loader's key check and is read as garbage. See "Native sample files
> and a boot project" at the end of this file.

## What `tools/firmware/plusdrive.py` builds

Given the above, a minimal image that should make the root directory
enumerate and be loadable:

1. Header sector (block 0): the four verified bytes, rest zero (the leaked
   region is not part of the format and zero is a valid substitute).
2. Table sector (block 0x800): all zero (nothing allocated in the three
   pools — we don't touch them).
3. Record-id bitmap (`0x5d8040`): bits set for id 2 (root) and one id per
   sample file.
4. Content-page bitmap (`0x5d80c0`-`0x5d8180`): bits set for every physical
   page used (root's content page, root's `0x10001` index page, each
   sample's data page(s)).
5. Record array (`0x5d8180`): root's 128-byte record (parent=2, one extent
   for its content page, one extent for its `0x10001` index page at logical
   page `0x10001`) and one 128-byte record per sample (parent=2, size=file
   length, one extent covering however many contiguous 32 KB pages the file
   needs, laid out contiguously so a single extent triple suffices even for
   multi-page files).
6. Root's content page: the directory-entry list (one variable-length entry
   per sample, per the table above).
7. Root's `0x10001` index page: `count` + one packed `(page,offset)` pointer
   per entry, in the same order as the content page.
8. Each sample's data page(s): the input WAV file's bytes, verbatim.

**Explicitly not attempted** (deferred pending emulator feedback): the
`0x10000` name-hash and `0x10002` id-sorted index pages (only needed for
by-name path resolution — `FUN_401572e0` — which on-device browsing/loading
may not exercise, since the browser already holds each entry's id from
enumeration and can plausibly open by id directly); this is the first thing
to add back if the emulator run shows a path-based open failing. The exact
attribute-byte values for "regular file" vs "directory" at record offsets
0x00/0x01/0x0c (table above) are also inferred, not proven — if the browser
misclassifies our sample entries, trace `FileSystemDirectory::vfunc_7/9/14`
(`0x40159380`/`0x40159466`/`0x401594ce`) against a value sweep.

> **Corrected 2026-09-28 [C][V].** Superseded. The bitmaps are big-endian
> u32 words (items 3-4 set the wrong bits: the firmware saw ids 26/27
> instead of 2/3), the directory needs ".", ".." and all three index pages,
> file content must start at page `0x78`, and samples are converted to the
> native format. See "Native sample files and a boot project" at the end of
> this file.

## Region 2 has a third, undocumented on-disk structure: a superblock at sector 0x5D8000 **[V]**

Neither this document nor `tools/firmware/plusdrive.py` accounted for this before now.
The real filesystem's mount routine, `FUN_4015a450`
(`out/ghidra/dt2-1.16-emac/decomp/4015a450_FUN_4015a450.c`, called from the
boot task `FUN_400cc864` at call site `0x400ccc00`, guarded by
`DAT_4029e9b0 & 0x10`), is the actual "is this card already mounted, real
filesystem" check (as opposed to `FUN_4015a424`/`FUN_4015a164(1)`, the
unconditional (re)format path, called instead when that bit is set):

```c
undefined4 FUN_4015a450(undefined4 param_1) {
  _DAT_44f2bd68 = 0;                                    // mount flag
  FUN_4012deda(0x5d8000, 0x200, &DAT_46f4dcb0);         // CMD18 read, 512 B
  if (_DAT_46f4dcb0 == 0x656b4653                        // magic
      && FUN_4015abb2(&DAT_46f4dcb0,0x1fc,0x31323334) == _DAT_46f4deac  // checksum
      && _DAT_46f4dcb4 - 3U < 2                          // version byte in {3,4}
      && FUN_4015a124(param_1) >= 0) {
    _DAT_44f2bd68 = 1;                                   // mount flag = mounted
    ...
    return 0;
  }
  return 0xffffffff;
}
```

The corresponding writer, `FUN_4015a164` (called via `FUN_4015a424`,
`docs`/finding-07's branch (A)/(B) format path), builds this exact 512-byte
buffer in RAM at `&DAT_46f4dcb0` and writes it to sector `0x5D8000` with the
CMD25 primitive (`FUN_4012e0c0`) at the very end of formatting:

| offset | value | meaning |
|---|---|---|
| 0x00 | `0x656B4653` | magic |
| 0x04 | `4` | version (mount accepts 3 or 4) |
| 0x08 | `0x8000` | = `PAGE` (32 KiB) |
| 0x0c | `0x58000` | |
| 0x10 | `0xA0080` | |
| 0x14 | `0x40` | |
| 0x18 | `0xC0` | |
| 0x1c | `0x180` | |
| 0x20 | `0x16180` | |
| 0x24 | `0x40` | |
| 0x28 | `0x20` | |
| 0x2c | value from `FUN_40155698(0x2c,...)` | |
| 0x30 | value from `FUN_40155698(0x2c,...)` (called twice; second call's value) | |
| 0x1fc-0x1ff | not covered by the checksum | reserved/unused by the check |

Fields at 0x2c/0x30 were not independently re-derived (they come from a
runtime call, not a literal); the rest are literals confirmed directly in
`FUN_4015a164`'s decompilation. **[O]**: exact meaning of most fields beyond
0x00/0x04/0x08 is inferred from context (block/page-count-shaped values),
not proven field-by-field.

**The checksum at 0x2c (RAM-cached as `_DAT_46f4deac`, computed over bytes
`[0x00:0x1FC]` of the buffer, seed `0x31323334`) is Bob Jenkins' public-domain
`lookup3.c` `hashlittle` (see `docs/sharc/SOURCES.md`-style citation: Bob
Jenkins, "lookup3.c", public domain, <https://burtleburtle.net/bob/c/lookup3.c>)
-- not a vendor algorithm.** Confirmed by matching, instruction-for-
instruction, the exact rotate constants of Jenkins' `mix()` macro (4, 6, 8,
16, 19, 4) in the streaming/block path (`FUN_4015aa20`) and `final()` (14,
11, 25, 16, 4, 14, 24) in the finalizer (`FUN_4015a6dc`), and the classic
seed pattern `a=b=c=seed+0xDEADBEEF` (`-0x21524111 == 0xDEADBEEF`) in
`FUN_4015abb2`'s init. For our fixed 508-byte (`0x1FC`) input this is the
"aligned, length%4==0" path: 42 full 12-byte `mix()` rounds, one trailing
4-byte word folded into `c` (case 4 of `FUN_4015a6dc`'s switch), then
`final()`. **[V]**: the algorithm identification is solid (the constants are
exact and too specific to be coincidence); a byte-exact Python
reimplementation has not yet been written or round-tripped against a real
firmware-computed checksum (needs a card that actually reaches this code
path -- see finding 07's new stall section -- to capture ground truth).

**Consequence for `tools/firmware/plusdrive.py`:** even once the finding-07 boot
stall is fixed, `FUN_4015a450` will reject any image this tool builds today,
because sector `0x5D8000` is all zero (never written). The tool needs to
also write this superblock, with a byte-exact `hashlittle` implementation,
for the real mount path to ever set `_DAT_44f2bd68 = 1`.

**Implemented.** `tools/firmware/plusdrive.py`'s `hashlittle()`/`build_superblock()`
now write this sector; `build()` calls it unconditionally. The checksum
routine was transliterated instruction-for-instruction from
`FUN_4015aa20`/`FUN_4015a6dc` (not reconstructed from the textbook
lookup3.c, since this variant's seed step omits the length term the
textbook one-shot version adds) and checked bit-for-bit against 44 real
calls to the firmware's own `FUN_4015abb2`, executed via a bounded
`emu.harness.call` on `snapshots/dt2-1.16/running.snap` -- lengths 0-29,
508 (the real superblock range), 511, 512, 600, 1199-1201 (either side of
`FUN_4015aa20`'s 0x4b0-byte streaming-chunk boundary) and 1211, 2401, with
random content. Every result matched. Two firmware quirks this uncovered,
both reproduced in the Python: (1) the seed is `initval + 0xDEADBEEF` with
**no length folded in**, unlike lookup3.c's one-shot `hashlittle`; (2) a
zero-length input's tail `switch` jumps straight past `final()`, returning
the raw seed unfinalized -- `tests/firmware/test_plusdrive.py` has the golden
vectors and the finalize-skip test. **[V]**.

Fields at superblock offsets `0x2c`/`0x30` (table above) are now also
resolved: they are two runtime-allocated physical sector numbers, each the
start of a 44-page run reserved by two calls to `FUN_40155698(0x2c,...)` in
the writer. Their consumer, `FUN_4015ae12` (run at both format- and
mount-time, right after the magic/checksum/version check succeeds), scans
exactly the sector range `[0x5ee980, 0x5eff80)` -- 88 pages, i.e. the same
two consecutive 44-page runs -- so the literal values are `0x5ee980` and
`0x5ef480` (confirmed by exact arithmetic, not just plausible-shaped
numbers). What that scan is *for* remains open (see below); it doesn't
block mount or a correct root listing either way, since our image leaves
that whole range zero (all-zero words read as "nothing cached", not a
crash -- `FUN_4015ae12` only extracts bit-0-set words into a lookup table).
Fields `0x24`/`0x28` are writer-side literals, not runtime-derived:
`0x24 = PAGE / SECTOR` (sectors per 32 KiB page) and `0x28` is the literal
argument the writer itself passes to one `FUN_40155698` call, kept as-is.
None of these five fields (`0x24`/`0x28`/`0x2c`/`0x30`, plus the whole
middle of the sector) are read by the mount path itself
(`FUN_4015a450`/`FUN_4015a124` only check magic/version/checksum), so
getting them wrong cannot fail a mount -- they're written for fidelity to
the real format, not because mount depends on them. **[V]** for what reads
them and where the values come from; **[O]** still for what the
`FUN_4015ae12` scan's cached table is actually used for downstream.

> **Corrected 2026-09-28 [C][V].** `FUN_4015ae12` scans only the first
> 44-page run, `[0x5ee980, 0x5ef480)` (pages `0x20`-`0x4B`), not 88 pages.
> It is the content-hash index (`hash|1` per record id, see "Native sample
> files and a boot project" below). The second run (`0x4C`-`0x77`) is
> reserved and zero-filled by the format; its use is **[O]**.

## The capacity bound fixed: `FUN_4015a450` genuinely mounts on a cold boot **[V][C]**

`docs/findings/07-emulator.md`'s "Booting with an already-formatted card
stalls" section pinned the remaining blocker down to `FUN_4012deda`'s bounds
check against `DAT_44e3fea0` (`0x3B0000` sectors, chosen to satisfy the
eMMC-identity whitelist), which rejects every sector `>= 0x3B0000` --
including the +Drive superblock at `0x5D8000`. This session fixed it, and
**corrects** that file's own closing paragraph: it is no longer true that
"no card-image boot has ever mounted +Drive's real filesystem in this
project".

**The fix is not simply "increase the capacity constant".** The first
attempt tried reaching `0x00760000` sectors (the whitelist's other literal
constant, `tools/firmware/plusdrive.py`'s own default image size) through the CSD-1.0
fallback formula `FUN_4012d4b2` already used for the smaller constant, by
raising `READ_BL_LEN` from 10 to 11. This is wrong, confirmed by an
instrumented live cold boot, not just by reading the decompile:
`FUN_4012d4b2`'s last step is `asr.l #9,D1` (an ARITHMETIC, not logical,
shift) on the capacity-in-bytes value built up to that point. `0x00760000`
sectors is `3,959,422,976` bytes, which is `>= 2^31` -- so that
intermediate value's own bit 31 is set, and the arithmetic shift
sign-extends instead of shifting in zeroes. Live result: `DAT_44e3fea0` read
back `0xFFF60000`, not `0x00760000` (exactly what sign-extending
`-655,360`, the byte-capacity reinterpreted as negative, right by 9
produces). **This is a real firmware limitation, not an emulator bug**:
CSD-1.0's `C_SIZE`/`C_SIZE_MULT` addressing tops out well under 2 GiB in the
real JEDEC spec too, which is exactly why EXT_CSD's `SEC_COUNT` field
exists for larger cards -- consistent with real hardware, this firmware's
CSD-1.0 fallback path simply cannot represent a capacity at or above 2 GiB.

**The correct path is EXT_CSD's `SEC_COUNT` field**, which an earlier
session had already tried and reverted, citing a real cold-boot hang
attributed to "some other, not yet located consumer" of those same 4 bytes.
`xrefs.sqlite` now names that consumer: `FUN_4012dca0`, a second,
independent identity/capacity check (re-reads `_DAT_4fe691d4` itself, not
just via `FUN_4012dc80`) that, whenever `SLC_OK` (EXT_CSD byte `0x98`) is
anything other than `1`, issues its own raw CMD6/SWITCH sequence directly
over the eSDHC MMIO registers and busy-waits on `PRSSTAT` bit `0x18` after
each write -- a sequence this project's `Esdhc` model does not implement
(nothing here ever sets that bit in response to a raw SWITCH command), so
any boot path that reaches it with `SLC_OK != 1` would spin forever. Both of
`FUN_4012dca0`'s own static callers are either debug-console-only
(`FUN_400cae8c`, the same "resample raw PCM" console handler this file
already names) or an indirect function-pointer reference with no resolved
static call site (`FUN_4010a54e`; `tools/coldfire/refscan.py` over the whole image
found only internal branches and the address-load itself, no call
instruction) -- consistent with `FUN_4012dca0` not being part of the normal
boot path. An instrumented live cold boot with `SLC_OK=0` (required to
select the whitelist row's larger capacity constant, see below) confirmed
this directly: `FUN_4012dca0` and its `PRSSTAT`-wait loop were never
entered through Main-OS task creation and hundreds of millions of
instructions beyond it. **[O]**: not exhaustively proven unreachable from
every boot path (an indirect call through a function pointer this project
hasn't traced) -- if a future run ever hangs spinning on `PRSSTAT` bit
`0x18` right after an eSDHC command, the fix is to model that SWITCH/
`PRSSTAT` sequence in `Esdhc`, not to revert `SLC_OK`.

`FUN_4012dbe0`'s own capacity compare (the eMMC-identity whitelist's third
check, see the section above) reads the matched row's raw bytes directly at
offsets 0x18 (`0x00760000`) and 0x1c (`0x003B0000`) and picks between them
via `SLC_OK`: byte `0` selects offset 0x18, byte `1` selects 0x1c (disproven
by disassembling `FUN_4012da80`, not by inspection of the row struct alone).
So reaching the larger constant needs `SLC_OK=0` (not `1`) alongside
`SEC_COUNT=0x00760000`, packed as a plain native big-endian 32-bit value --
no shifting, so it carries none of the CSD path's overflow risk.

**`emu/esdhc.py` now derives both together from one table**
(`_CAPACITY_PARAMS`), keyed by the target capacity in sectors, so
`SEC_COUNT`/`SLC_OK` (for the larger, `0x00760000`-sector constant this
model defaults to) and the CSD-1.0 fallback fields (for the smaller,
`0x003B0000`-sector constant, kept at their original, already-verified
encoding) can never drift out of sync. `Card.from_file()` derives
`capacity_blocks` from the image file's own size, so a default
`tools/firmware/plusdrive.py` build (exactly `0x00760000` sectors) reports the
capacity the whitelist demands automatically; any other size now raises a
clear `ValueError` instead of silently booting a card the real whitelist
would reject.

**Verified two ways.** A bounded, isolated call to the real `FUN_4012dc80`
(`tests/test_esdhc_identity.py`, extended to reproduce
`FUN_4012d4b2`'s real SEC_COUNT-vs-CSD-fallback branch rather than assuming
the CSD-1.0 formula alone) returns `D0=0`. An instrumented live cold boot
(`emu.dspboot.run` with `--card-image out/plusdrive/dt2.img`, scoped code
hooks, no `--card-image`-driven resume) confirms the same live: `D0=0` right
after `FUN_4012dc80`, `DAT_44e3fea0` reads back `0x00760000` correctly, the
superblock CMD18 at sector `0x5D8000` is issued and accepted
(`payload_available=True`), the factory-table CMD18 at `0x458000` likewise,
and **`_DAT_44f2bd68` (the mount flag) reads `1`** -- the first time this
project has observed a real, successful `FUN_4015a450` mount. The boot task
then falls into its own known terminal idle loop (`0x400cccd8`, the same
address `docs/findings/07-emulator.md` already names as this task's normal
end state, not a hang) having done real, further filesystem-level eSDHC
traffic (reads/writes of the record-id and page-occupancy bitmaps, the
record area, and other pool/cache sectors) beyond the mount check itself.

**This does not, on its own, reach `docs/findings/07-emulator.md`'s
"running" state (PIT3/DTIM3 firing, vector 208 handed to the real display
ISR).** A continued run to 780M total instructions shows zero further eSDHC
traffic and zero new tasks created after ~n=260M, `vec208` still equal to
the intro's own PIT3 ISR, and the boot task still parked at `0x400cccd8`.
This is that file's own already-documented, separately-scoped branch-(C)
display-start gap ("Update MMC Caches" -- the header this project's header
now validly forces, per the header-state decision tree in that file's
"Booting with an already-formatted card stalls" section) -- not a
regression from this fix, and not something this session chased further:
it was open before this fix and remains open after it, unrelated to +Drive
or card capacity. **[O]**: locating branch (C)'s real display-start
mechanism remains the blocker for observing the +Drive browser end-to-end
in a genuinely "running" emulator state.

## `SampleManager` holds two `Directory` implementations, not one **[D]**

`SampleManager`'s real constructor is Ghidra-mislabeled as
`std::_Sp_counted_ptr_inplace<SamplePoolDirectory,...>::ctor_dtor`
(`out/ghidra/dt2-1.16-emac/decomp/40029e7a_...ctor_dtor.c`; its own leading
comment says it "loads the vtables of SampleManager,
`_Sp_counted_ptr_inplace<FileSystemDirectory,...>`,
`_Sp_counted_ptr_inplace<SamplePoolDirectory,...>`" -- i.e. this function
*is* `SampleManager::SampleManager`, not `SamplePoolDirectory`'s). It
constructs **both**:

- a `FileSystemDirectory` (the real, path-addressable filesystem this
  document and `tools/firmware/plusdrive.py` target -- region 2), stored at
  `this+0x81`/`+0x82` (a `shared_ptr` pair), and
- a `SamplePoolDirectory` (a *different* class from the `MmcFs` pools --
  region 1), stored at `this+0x83`/`+0x84`,

then picks **one** of the two as "active" (`this+0x7f`) based on a boolean
(`param_5` in the deepest constructor signature) that could not be pinned
to a concrete literal by static reading alone: the intermediate forwarding
layer, `std::_Sp_counted_ptr_inplace<SampleManager,...>::ctor_dtor`
(`0x4019baa6`), reads its own extra arguments via `in_stack_...`
pseudo-variables that Ghidra's decompiled C does not show being passed by
its only caller, `FUN_4019bb28` -- confirmed by reading
`out/ghidra/dt2-1.16-emac/disasm/4019bb28_FUN_4019bb28.s` directly: the
decompiled C shows one literal `0` argument where the raw disassembly pushes
several more stack words. **This mismatch means the decompiled call-argument
counts for this whole `_Sp_counted_ptr_inplace<X>::ctor_dtor` family cannot
be trusted without checking the disassembly.** [O]: which `Directory` is
actually active for the sample browser -- static reading was inconclusive;
needs a live read of `this+0x7f` against `+0x81`/`+0x83` in a booted
snapshot (blocked on finding 07's boot stall).

`FileSystemDirectory::ctor_dtor` itself
(`out/ghidra/dt2-1.16-emac/decomp/40159ca0_FileSystemDirectory__ctor_dtor.c`)
does no disk I/O -- it just sets the directory's current id (via
`FUN_401598ec`, presumably `2`, the root) -- so construction alone proves
nothing about mount state; readdir only happens on actual navigation
(`FUN_40157072`, already in this document).

## Open questions

- Exact semantics of record offsets 0x00 bit0, 0x01 bit0, 0x0c bit0
  (candidates: is-directory, protected/read-only, some third flag) — not
  disambiguated from MAIN_OS code alone. *2026-09-28:* word +0x0C is the
  content hash with bit 0 set (`hash|1`); the loader's key check needs bit
  0 **[V]**.
- The RAM-resident record ids (`0x1000000`-`0x1000800`, table at
  `0x47d73ed0`) and RAM-resident page ids (same range for pages, table at
  `0x46f63ed0`) — not traced; unclear if they're a boot-time cache of disk
  content or synthetic pseudo-entries, and whether the sample path ever uses
  them.
- The second content-page region at sector base `0x458000` — purpose
  unresolved.
- `0x10001` index page's exact byte layout beyond the count field and the
  4-byte-per-entry packed pointer array — inferred from the read pattern,
  not cross-checked against a real populated directory. *Answered
  2026-09-28 [C][V]: 8-byte entries from byte 8, see the last section.*
- The RPC-opcode dispatcher connecting `elektron::MidiRpcFsSample
  WriteFileV1/V2`/`CreateDir` (whose own vfuncs are ctor/dtor boilerplate)
  to the generic `File`/`FileSystemDirectory` layer — not located; doesn't
  change the "raw write, no interpretation" conclusion since `File::write`
  is the only place bytes could be inspected.
- On-disk sample payload format (verbatim WAV vs. something converted at
  load time) — the single biggest open question; needs an emulator trace of
  an actual "load sample from browser into track" action, or the untraced
  `SampleManager::vfunc_30`-`vfunc_45` / `SampleLoaderBgWorker` read path.
  *Answered 2026-09-28 [C][V]: the native format, see the last section.*
- Whether firmware ever re-validates the two filesystem bitmaps
  (`0x5d8040`, `0x5d80c0`-`0x5d8180`) against record/extent data outside the
  allocator (a mount-time consistency check) — not searched for.
- The sector-`0x5D8000` superblock's fields beyond magic/version, and a
  byte-exact `hashlittle` implementation for `tools/firmware/plusdrive.py` to write
  a valid one — **answered**, see the new section above: implemented and
  checked bit-for-bit against 44 real firmware executions. `out/plusdrive/dt2.img`
  now carries a superblock that self-verifies (`build_superblock()`'s own
  checksum recomputes to the same value; confirmed against a freshly
  rebuilt image with `tools/firmware/plusdrive.py ls`).
- **`FUN_4015a450` (mount) was never called at all on a `--card-image`
  boot, independent of the superblock's correctness — this is now fixed
  and verified, but a separate, deeper emulator hang was uncovered right
  behind it.** `FUN_400cc864` (the boot task that calls `FUN_4015a450`/
  `FUN_4015a424`) gates that whole call behind `_DAT_42940a48 == 0`, set
  from an eMMC identity check (`FUN_4012dc80`→`FUN_4012db90`[build a
  struct]→`FUN_4012dbe0`→`FUN_4012da2c`×2, a scan of a 7-entry
  manufacturer/product-name whitelist at `0x402b4a24`-`0x402b4b04`, values
  `0x11`×4/`0x15`/`0x70`×2, names like `"004GE0"`, eMMC part numbers).

  **The fix, confirmed correct by execution.** Disassembling (not
  decompiling) `FUN_4012db90`/`FUN_4012d4b2`/`FUN_4012db32` found the
  decompiler had misread the manufacturer-ID read as `_DAT_44e3fe9c & 0xff`
  (a byte mask on a 32-bit load); the real instruction is `mvz.w
  (DAT_44e3fe9c).l,D0` (a 16-bit load) then `mvz.b D0b,D0` (keep the word's
  low byte) — i.e. the byte actually compared is at `0x44e3fe9d`, which is
  CID RSP3 bits `[23:16]`, exactly where a real CID's manufacturer ID
  (JEDEC CID\[127:120\]) lives. `emu/esdhc.py`'s existing
  `self.cid = [0, 0, 0, 0x00110000]` already put `0x11` there — so the
  manufacturer half of a prior, incomplete fix attempt was already right;
  only the product name was missing. The product name is 6 ASCII chars,
  RSP2's 4 bytes followed by RSP1's top 2 bytes (`FUN_4012db32`,
  confirmed by disassembly), compared with `FUN_40189ff4` (literally
  glibc's word-at-a-time `strcmp`). `emu/esdhc.py`'s `Card.cid` now encodes
  `"004GE0"` there (`self.cid = [0, 0x45300000, 0x30303447, 0x00110000]`),
  one of the four valid names for manufacturer `0x11`.

  A second, nested check inside `FUN_4012dbe0` (only reached once the first
  matches) compares the caller's own struct fields 2-4 -- a 24-bit
  big-endian pack of EXT_CSD bytes `0x9C:0x9D:0x9E` (`FUN_4012db90`) plus
  bytes `0xDE`/`0xE3` -- against the *same matched row*'s own fields 2-4
  (`0x1D8`/`0x01`/`0x08` for "004GE0"); `emu/esdhc.py`'s `ext_csd()` now
  sets those three fields (previously a single `PARTITIONS_ATTRIBUTE` byte
  fixed at `1`). A third, final check compares a computed capacity against
  one of that row's own two capacity constants (`0x760000` or `0x3B0000`
  sectors), selected by EXT_CSD byte `0x98` (`FUN_4012da80`; previously
  modeled, under the name `SLC_OK`, as an unrelated "MMC NOT IN SLC MODE"
  flag some *other*, now-stale-cited address wanted `1` for -- see
  `emu/esdhc.py`'s `SLC_OK` comment for the full correction). **The
  capacity constant can't be reached through EXT_CSD's real `SEC_COUNT`
  field**: encoding it there so a native 32-bit big-endian load recovers
  `0x760000` (this project's existing default card capacity, needed since
  it's smaller than +Drive's own layout) made a real cold boot **hang**
  outright, confirmed by bisection (`emu.dspboot.run` directly, reverting
  only that one field's byte order) -- some other consumer of the same 4
  bytes evidently depends on the JEDEC byte-serial (little-endian)
  convention this project's `SEC_COUNT` already used, and a ~30x larger
  value there stalls something indefinitely. So EXT_CSD's `SEC_COUNT` is
  now left at a fixed `0` (a real, if less common, eMMC behaviour: no
  extended-capacity report), and the smaller capacity constant
  (`0x3B0000`) is supplied instead through the CSD-1.0 fallback formula
  `FUN_4012d4b2` computes from `_DAT_44e3fe84`/`fe88` (a second CMD9/
  SEND_CSD copy) when `SEC_COUNT` reads as 0 -- reproduced exactly (down to
  matching the disassembled bit-shift sequence in Python) and set via
  `Card.csd`'s `CSD_RSP1`/`CSD_RSP2`. **Verified this whole chain is
  correct with a bounded, isolated call to the real `FUN_4012dc80`**
  (`tests/test_esdhc_identity.py`, gated on `DT2_SYX` since it needs the
  real firmware image): built from `emu.esdhc.Card()`'s own live CID/
  EXT_CSD/CSD values (not hand duplicated), it returns D0=0. A live,
  instrumented cold boot (code hooks on every function in the chain, not
  just the unit test) confirms the *exact same* result happens for real:
  `FUN_4012dc80` runs to completion and returns 0, having read
  `fe7c=1 fea0=0x3b0000 fe88=0xa03 fe84=0xafc00380` -- byte-for-byte the
  intended values -- immediately after a real `CMD9`/SEND_CSD exchange.
  **This part of the fix is done and correct.**

  **The boot-task hang immediately after the identity check is now fixed --
  root cause was an emulator wiring gap, not firmware or image content.**

  Two corrections to this file's own earlier passes at this, from
  disassembling the real code (addresses were off by exactly `0x400` in an
  earlier pass, and a `bra.b $-2` at `0x400CC7AE` belongs to an unrelated,
  low-priority task idling normally, confirmed by reading its own TCB live
  -- not evidence of a hang by itself). The real call sequence, read from
  `out/ghidra/dt2-1.16-emac/disasm/400cc864_FUN_400cc864.s`, is: identity
  check succeeds -> `_DAT_42940a48` stored at `0x400ccb5a` -> `FUN_400f0628`
  (`0x400ccb6a`) -> ... -> `FUN_4015a43a`/`FUN_4015a450` (`0x400ccbe6`/
  `0x400ccc00`). An instrumented boot following the boot task's own TCB
  (not a whole-image PC sample) showed it enter `FUN_400f0628`, issue a
  `CMD18(0x40000)` read, and then legitimately block in `sem_pend`
  (`0x4000141a`) forever. `tools/guirun.py`'s own `sem_pend`/`pend_b` hook
  pattern, reused directly, named the exact semaphore: after several
  ordinary `cmd_sem`/`data_sem` waits that all resolved normally, the last
  pend was on `sd_dma_sem` (`0x44e3feb8`) from inside `FUN_4012e0c0` (the
  CMD25/multi-block-WRITE primitive) -- a **write's** DMA completion that
  never arrived.

  **Root cause: `emu/dspboot.py`'s own `Esdhc(...)` construction never
  passed `dma_sem`.** `emu/longrun.py`'s construction of the same class
  always has (`dma_sem=profile.sd_dma_sem`); `emu/dspboot.py`'s (used only
  for the *initial* cold-boot pass, i.e. exactly where this hang occurs)
  passed `cmd_sem`/`data_sem` but silently left `dma_sem` at its `None`
  default. `Esdhc._post(None)` is a documented no-op (see its own
  docstring), so the guest's real eDMA-channel-59-completion `give` this
  model performs on a write's behalf (see `vector_192_handler`'s own
  disassembly below) never lands, and any multi-block CMD25 write's own
  `sem_pend(sd_dma_sem)` -- reads, which pend on `data_sem` instead, are
  unaffected -- blocks forever. **Fixed**: pass `dma_sem=profile.sd_dma_sem`
  in `emu/dspboot.py`, matching `emu/longrun.py`. New regression test,
  `tests/test_dspboot_esdhc_wiring.py` (`DT2_SYX`-gated); the `Esdhc` model's
  own CMD25-posts-`dma_sem` behavior was already covered by
  `tests/test_esdhc.py`'s `test_cmd25_consumes_host_buffer_and_is_visible_to_cmd18`
  -- this was purely a construction-site wiring gap, not a model bug.

  For completeness (per the coordinator's own ask), the real firmware's
  eDMA-channel-59 completion ISR, `vector_192_handler`
  (`out/ghidra/dt2-1.16-emac/disasm/4012d3c4_vector_192_handler.s`, 40
  bytes) is trivial: `move.b #0x3b,(EDMA_CINT)` (acks/clears channel 59's
  own interrupt request) then unconditionally `give_b(sd_dma_sem)` -- no
  IRQSTAT/TCD inspection at all, confirming this model's own
  synchronous-completion shortcut (posting straight from `Esdhc.command()`,
  never actually raising vector 192 -- see `emu/longrun.py`'s own comment
  on this) is a faithful enough simplification of what the real ISR does;
  the bug was purely that this specific construction site left the target
  semaphore address as `None`.

  **With this fixed, the boot progresses dramatically further** (a cold
  ladder to 400M instructions now creates 10 tasks, including the
  priority-6 Main-OS task, `entry=0x400337ba`, that finding 07's own
  branch-A/B/C investigation names) **and `_DAT_42940a48` (the identity
  check's own gate) reads `0` at the boot task's own terminal idle loop
  (`0x400cccd8`, confirmed unmoving after 1B total instructions) -- i.e.
  `FUN_4015a450` (mount) is now definitely called.** It still does not
  succeed: `_DAT_44f2bd68` (the mount flag) stays `0`.

  **A third, previously-undocumented on-disk structure blocks mount's own
  success, past the superblock at `0x5D8000` -- record now built and
  verified valid in isolation, but a live boot still doesn't show it being
  consumed, for a reason not yet found.** `FUN_4015a450`'s own
  `FUN_4015a124(0)` call (already read in this file's earlier section) goes
  on to call `FUN_4002cd6a`, which -- via `FUN_4002ccd0` -- reads **32 KiB
  from sector `0x458000`** (finding 14's own earlier table already flagged
  this exact sector as "purpose unresolved").

  **Fully pinned down by disassembly, not the decompile** (which had
  misread offsets 8/10 as one overlapping 32-bit field --
  `out/ghidra/dt2-1.16-emac/disasm/4002ccd0_FUN_4002ccd0.s` shows two
  separate `mvz.w` 16-bit reads): magic `0x4D61476A` ("MaGj") at offset 0;
  a 16-bit field at offset 10 == `3`; a 16-bit field at offset 8 == `0x2B`;
  then a two-stage CRC-32/IEEE (`FUN_4013e06c`, disassembled -- its
  per-byte update is the textbook table-driven `crc =
  table[(crc^byte)&0xff] ^ (crc>>8)`, no extra initial/final complement
  beyond whatever seed the caller passes): first pass seeded `0xFFFFFFFF`
  over `record[8:8+length]` where `length = record[12:16] - 8` (a
  self-describing size field, built here as `12`, so `length=4`, covering
  exactly the two checked 16-bit fields); second pass chained from that
  result over `record[4:8]`, expected to equal the literal `0xDEBB20E3`.
  `record[4:8]`'s value isn't checked directly, only through that chained
  CRC -- solved for (not guessed) via a GF(2) linear-algebra CRC inversion
  (`crc32_ieee_raw` is linear in its input bits for a fixed starting state,
  the same property CRC-combine tools use): `0xA7,0x27,0x55,0x8C`.
  **Verified against the real firmware**: `tools/firmware/plusdrive.py`'s
  `build_factory_table_record()` builds this record (the rest of the 32
  KiB left zero), and `tests/test_factory_table.py` (`DT2_SYX`-gated) makes
  a bounded, isolated call to the real `FUN_4002ccd0` with this exact
  record in RAM (bypassing the CMD18 read, the same technique
  `tests/test_esdhc_identity.py` used) and gets `D0=1` (accepted).

  **Still open**: once `FUN_4002ccd0` accepts, `FUN_4002cd6a`'s own success
  path goes on to iterate a ~1265-entry factory-drum-hit table
  (`FUN_4015bede`/`FUN_4002cc7e`, reading further into this same 32 KiB
  buffer) before returning success unconditionally -- this project has not
  verified that loop is safe against an otherwise-all-zero buffer (it
  might, itself, block on something, e.g. by trying to link
  garbage-addressed filesystem entries). A cold boot with the new record
  written (`out/plusdrive/dt2.img` rebuilt, ladder rebuilt to match) still
  shows the mount flag at `0` and the boot task already parked in its own
  known terminal idle loop after 1.6B total instructions -- i.e. `FUN_4015a450`
  should have already run and returned by then, but the RAM buffer
  `FUN_4002ccd0` reads into (`DAT_47e203cc`) reads back all zero afterward,
  meaning either `FUN_4002ccd0` was never actually reached in the live run
  (contradicting a straightforward read of `FUN_4015a124`'s own
  unconditional call chain), or its `CMD18` read of this unusually large
  (32 KiB / 64-sector) single request didn't deliver the image's bytes the
  way the isolated unit test's direct RAM write did. **[O]**: needs a
  session with scoped code hooks (fast; a global per-instruction hook is
  too slow to run for the ~1-1.6B instructions this boot needs) on
  `FUN_4015a450`/`FUN_4002cd6a`/`FUN_4002ccd0`, resumed from a snapshot
  taken shortly before the expected call (not from the very start), to see
  whether these are reached at all and, if so, what the live `CMD18`
  actually delivers into `DAT_47e203cc`. Until this is resolved, no
  card-image boot reaches a valid mount, and item 3 (browser lists
  `hat.wav`) still can't be demonstrated end-to-end.

  **[C]: resolved, in a later session -- see "The capacity bound fixed:
  `FUN_4015a450` genuinely mounts on a cold boot" above.** `FUN_4002ccd0`
  *was* being reached; its `CMD18` for the factory-table sector (and the
  superblock's) simply never reached the eSDHC hardware at all, rejected
  outright by `FUN_4012deda`'s capacity bounds check before any DMA -- a
  different root cause than either alternative this paragraph considered.
  With that fixed, the mount flag now reads `1` on a true cold boot. Full
  "running" (needed to demonstrate the browser listing `hat.wav`
  end-to-end) is still blocked, but by `docs/findings/07-emulator.md`'s
  separate, already-open branch-(C) display-start gap, not by this.
- Which `Directory` (`FileSystemDirectory` vs `SamplePoolDirectory`)
  `SampleManager` actually browses — **answered**: a live crash trace
  (see `docs/findings/07-emulator.md`'s corrected section below) found
  `SampleManager`'s "currently selected" item is a `FileSystemDirectory`
  instance (vtable `0x4022584c`) in both a cardless boot and a
  `--card-image` boot, and it is not yet `valid()` (its `vfunc_12`, a
  flag at offset `0x120`, reads false) in either case — i.e. this project
  has never observed a `FileSystemDirectory` that successfully mounted.
- Booting with *any* already-formatted card (ours or the firmware's own)
  currently stalls before `running` for a reason unrelated to +Drive
  content — see `docs/findings/07-emulator.md`'s new section. This blocks
  items 2 and 3 of the +Drive-in-RAM goal (reading the live `Directory` for
  `hat.wav`, saving a running card snapshot) until it is fixed.
- Separately, once `running` is reached, opening the sample-pool list or
  the SampleManager/+Drive browser screen itself (SRC, encoder, FUNC, YES)
  panics the UI task — also confirmed unrelated to +Drive content or the
  eSDHC model (identical crash with `--card-image` omitted). **Corrects
  an earlier framing in this file and in `docs/findings/07-emulator.md`**:
  this is not a resource/glyph decode-cache miss. It is an uncaught C++
  exception (`std::logic_error("basic_string::_S_construct null not
  valid")`, i.e. `SampleManager::vfunc_40` builds a `std::string` from a
  null `const char*` with no null check) thrown because the selected
  `FileSystemDirectory` reports itself invalid (see above) and its
  "get display name" accessor (`FUN_4015996e`) returns a raw `NULL` for
  that case instead of the empty-string fallback it already uses for its
  other failure case. See `docs/findings/07-emulator.md`'s "Opening the
  sample-pool list or the +Drive browser panics the UI task" section for
  the full root-cause trace and why this most likely traces back to the
  mount gap above. It still blocks reading the live `Directory` through
  this screen; a direct RAM read of the mounted `Directory` object
  (bypassing the browser UI) remains the open path to confirming `hat.wav`
  is listed.

## Native sample files and a boot project (1.16) **[V][D][C]**

2026-09-28. `tools/firmware/plusdrive.py` now writes samples in the firmware's own
format and a boot project whose slots reference them, and
`tools/plusdrive_check.py` checks an image with bounded calls of the
firmware's own functions. Marks: **[V]** re-read against the image bytes by
a second agent (verification lane, 2026-09-28); **[D]** read or executed
once. How the loader streams a file to the SHARC is in finding 04 ("Sample
data crosses FlexBus to link port 0 on 1.16") and
[15](15-sharc-sample-path.md).

    uv run python -m tools.firmware.plusdrive build samples -o out/plusdrive/native/dt2.img
    DT2_SYX=Digitakt_II_OS1.16.syx uv run python tools/plusdrive_check.py out/plusdrive/native/dt2.img   # ~80 s

### The native sample file **[V]**

Writer: the sampler's "Save recording" job, `FUN_400213f4` ->
`FUN_4001f8b0` -> `FUN_40153994`.

| offset | value |
|---|---|
| `0x00` | 0 |
| `0x01` | 1 = stereo (interleaved L/R), 0 = mono |
| `0x04` | u32 BE: PCM data length in bytes |
| `0x08` | u32 BE: sample rate (48000) |
| `0x14` | byte `0x7F` |
| rest of `0x40` | 0 |
| `0x40`.. | big-endian int16 PCM (from the wire byte swap, finding 04) |
| last 16 bytes | copied from `0x405a50a0` |

File size = data + 0x50. `0x405a50a0` is referenced only inside
`FUN_40153994`; the trailer's content is **[O]** (plusdrive writes zeros).
The loader reads byte 1, +4 and +8 of the header (finding 04) **[D]**.

*P0 2026-09-29.* The header was re-derived twice: from the Ghidra listing
and decompilation of `FUN_40153994`, and from a capstone decode of its raw
bytes (`0x40153a1e`-`0x40153a5a`: `memset(hdr, 0, 0x40)`, then +4 = byte
length, +1 = the stereo flag, +0xC = 0, +8 = `#$bb80`, +0x10 = 0, +0x14 =
`#$7f`; the file is sized with `pea $50(a0)`, a0 = byte length). The
trailer's only literal is at `0x40153afa`. The job chain was checked too:
`FUN_400213f4` passes `0x4001f8b0` as a functor (`move.l #$4001f8b0,d0` at
`0x4002154a`; cfdb has no caller, as expected for a functor), and
`FUN_4001f8b0` is `FUN_40153994`'s only caller. End to end, hat's file at
content page 0x7C of `out/plusdrive/native/dt2.img` has this header (length
1,316,576, 48000, stereo), and its PCM equals the FlexBus pages after the
wire swap (finding 04).

**Content hash [D].** After close, `FUN_4015b0ce` -> `FUN_4015af0c`
computes a hash over the file (seeds `0x43fa243a` x3, `FUN_4015aa20` /
`FUN_4015a6dc`) and stores `hash|1` in record word +0x0C and in the hash
table at sector `0x5ee980 + (id >> 13) * 0x40`, word `id & 0x1fff`.
`tools/firmware/plusdrive.py` computes the same value (the firmware's own
`FUN_4015af0c` rewrote hat's entry with the image's value).

### References and ids

- A sample reference is 16 bytes `{u32 id, u32 hash|1, u32 size, u32 seq}`
  (the loader's key built by `FUN_4015ab5c` from record +0x0C, +0x04,
  +0x10) **[V]**. `FUN_4015ab5c` returns -1 unless record +0x0C bit 0 is set
  **[V]**.
- `FUN_4015b178` resolves a ref: a valid id with matching seq, hash and size
  -> 1; seq mismatch but hash and size match -> 3; an invalid id (such as
  0) -> binary search of the hash index built at mount by `FUN_4015ae12`
  -> 4 **[D]**.
- `FUN_4015b4f0` reserves record ids 0-1; `FUN_4015a164` reserves content
  pages `0x00`-`0x77` (`0x00`-`0x1F`, then the two 44-page runs). File and
  directory pages start at `0x78` **[V]**.
- **[C]** The id and page bitmaps are arrays of big-endian u32 words, bit n
  = `1 << (n & 31)` of word `n >> 5` (`FUN_4015b93e`, `FUN_4015b7ac`,
  `FUN_40155698`) **[V]**. The old `set_bits` (byte `n >> 3`, bit `n & 7`)
  made the firmware see ids 26 and 27 instead of 2 and 3.

### Directories **[V][D]**

- **[C]** All three index pages `0x10000` (name hash), `0x10001` (order)
  and `0x10002` (id) hold 8-byte entries from byte 8, `{u32 key, u32
  page * 0x8000 + offset}` **[V]**. The `0x10001` key is the first four name
  bytes and the listing comparator (`FUN_40135df2`, approximately
  case-insensitive with digit runs by value) only affects order **[D]**.
- **[C]** A directory needs "." and ".." as its first two entries; the
  parent is the id at content byte 0x0C (`FUN_40155ea8`). `FUN_401575f6`
  (path of an id, used by the loader for the slot name) needs ".." and the
  `0x10002` page and returns NULL without them **[V]** (the loader then reads
  the name from address 0). `FUN_40156334` (link) uses the `0x10000` page
  unconditionally, and the mount links "factory" (id `0x1000000`) into the
  root every time **[D]**.
- A directory's size is whole pages (0x8000 each); the last entry's slot
  length runs to the page end **[D]**.

### The boot project **[V][D]**

- `FUN_400cc864` (boot task) -> `FUN_400f0628` reads 0x100 bytes from
  sector `0x40000` (fallback `0x48000`, table `0x40215d24`) into
  `0x4099d588` and checks the COKi header (magic `0x434F4B69`). If the flag
  word `0x4029e9b0 & 3 == 0` it reads `0xDD9714` bytes: a 0x110-byte header
  and the active project container at `0x4099d698` **[V]**.
- `FUN_400c0c2c` (called at `0x400ccbd6`) decodes it (`FUN_400e0fdc` ->
  `FUN_400e0c26`, magic `0xBEEFBACE`, upgraders v1..v5; end marker
  `0xBACEF00C` **[V]**). On failure it creates a new project **[D]**. The
  mount follows (`0x400ccc00`), then Main OS queues "Load all samples".
- The firmware holds a valid container: the built-in project at
  `0x4025f1d8` (and a byte-identical copy at `0x4027989c`) depacks to
  0xC3F424 bytes, `BE EF BA CE 00 00 00 03` (v3). At container offset
  `0xC2D6FB` (unaligned) is the 1024 x 16-byte reference table: entry 0 =
  `{0xFFFFFFFF, 0, 0, 0}`, 297 non-empty entries **[V]**; 286 with factory
  ids `0x1000000`-`0x10007FF`, 11 with id 0 **[V]** (P0 2026-09-29: a
  struct parse of the depacked table, separate from
  `plusdrive.container_refs`, gives 286 factory ids, 11 zero ids and 726
  empty entries `{0xFFFFFFFF, ...}`; the two copies are byte-identical
  over their whole packed length 0x1a6b9, and the sector table at
  `0x40215d24` reads `{0x40000, 0x48000}`).
- COKi header **[D]**: +0x104 is the record sequence counter
  (`FUN_4015b7ac`), +0x18 is set to the superblock version at mount
  (`FUN_400c0eea` -> `FUN_400c0ec8`), +0x14 bit 1 -> flags `|= 0x92`, bit 0
  -> `|= 2`, and the container is read only if flags & 3 == 0
  (`FUN_400f0628`). Bytes 0x10-0x110 otherwise are **[O]** (plusdrive
  writes zeros).
- In the v3 kit, a track's int16 slot is at `+0x3C + t*0x155 + 0x64` and a
  copy of the slot's reference at `+0x129`. Kit 0 ("KIT 1") is active; its
  track 1 uses slot 7 **[D]**.

### What `tools/firmware/plusdrive.py` builds now **[D]**

For `samples/hat.wav` (24-bit stereo 44.1 kHz, 302,401 frames): 24 -> 16
bit by rounding, 44.1 -> 48 kHz by a Kaiser-windowed sinc (beta 8.6, 32
taps each side), 329,144 frames, no clipping; a 1,316,656-byte native file,
record id 3, name "hat", content pages `0x7C`-`0xA4`, record +0x0C =
`0x7AFE4EDD`, seq 3. Root directory: content page `0x78`, index pages
`0x79`-`0x7B`. Project record at `0x40000` and `0x48000`: a 0x110-byte COKi
header (+0x104 = 4, +0x18 = 4, +0x14 = 0) and the built-in container
(depacked at build time from `out/sections/dt2-1.16/`, never committed)
with all 297 slot references and 1,471 track copies replaced by hat's,
padded to 0xDD9714 bytes. `--no-project` and `--main-os` exist.
(P0 2026-09-29 **[V]** for the file: 329,144 frames, 1,316,576 data bytes
+ 0x50 = 1,316,656, 41 content pages from `0x7C` to `0xA4`, read back from
the image.)

`tools/plusdrive_check.py` on that image (bounded calls on
`snapshots/dt2-1.16/running.snap`; eMMC reads served from the image, writes
to an overlay, mutexes no-ops, `FUN_400cd638` and `FUN_40153622` recorded):

| check | result |
|---|---|
| mount `FUN_4015a450(0)` | D0 0, mount flag 1 |
| id bitmap `FUN_4015b93e(3)`, hash index (`FUN_4015ae12`, `FUN_4015a94c`) | valid; one entry `{0x7afe4edd, 3}` |
| readdir `FUN_40157072`, lookup `FUN_401572e0(2, "hat")`, name hash `FUN_40155f96`, path `FUN_401575f6(3)` | ".", "..", "factory", "hat"; id 3; hashes equal Python's; "/hat" |
| resolve `FUN_4015b178`, key `FUN_4015abe6` | 1 (exact), id 3; key = the ref |
| COKi check and decode `FUN_400c0c2c(0)` | valid, no new project; container upgraded v3 -> v5 |
| slot list `FUN_4004e598` | 7, 1, 3, 8, 9, 12, 4, ...; the 16 tracks' (machine, slot) as in finding 04 |
| `FUN_40154540(ref, 7)` | 322 data pages; headers `{7, 0xfffffc00, 0xfffffc00, 0, 0}` then `{7, 0x20, 0xa1020, 48000, 0xa0b70}`; L and R PCM equal the file |
| `FUN_40154540(ref, 1)` | alias: header only |
| `FUN_4015af0c(3)` | rewrites `0x7afe4edd`, the image's value |

`tests/test_plusdrive_firmware.py` (slow, needs `DT2_SYX` and
`snapshots/dt2-1.16/running.snap`) runs the same checks on a synthetic
stereo WAV and pins track 1 = (ONESHOT, slot 7). A cold boot with this
image loads the sample (finding 07, "A +Drive cold boot loads a sample end
to end").

Open **[O]**: the 16-byte trailer; COKi header bytes 0x10-0x110; whether UI
or save code dislikes 297 identical references; one content page per
directory and one extent per file (fine for a few hundred files; larger
inputs raise).

## The pSLC card identity: no "MMC NOT IN SLC MODE" dialog (2026-10-06)

**Identity model.** `packages/emulator/card` (`CardIdentity`) and `emu/esdhc.py`
(`IDENTITIES`) now model three firmware-whitelisted eMMCs; the identity, not
the backing image length, sets CID, EXT_CSD and capacity. `DIGI_CARD_IDENTITY`
selects it (`slc32g` default, `mlc4g`, `small`).

| identity | CID manufacturer / name | EXT_CSD 0x98 | capacity (sectors) |
|---|---|---|---|
| `slc32g` (default) | 0x70 "TX2932" | 1 | 0x2e1a000 |
| `mlc4g` (the old default) | 0x11 "004GE0" | 0 | 0x760000 |
| `small` | 0x11 "004GE0" | 1 | CSD fallback, 0x3b0000 |

- EXT_CSD is written in the guest-native layout, JEDEC index XOR 3: guest
  0x98 = PARTITION_SETTING_COMPLETED, 0x9c..0x9e = MAX_ENH_SIZE_MULT, 0xde =
  HC_WP_GRP_SIZE, 0xe3 = HC_ERASE_GRP_SIZE, 0xd4..0xd7 = SEC_COUNT (a plain
  big-endian word for `slc32g` and `mlc4g`; 0 for `small`, which selects the
  CSD fallback). **[D]** from `emu/esdhc.py`; the whitelist rows below were
  taken from the coder's notes, not re-read from the image here.
- The whitelist (same table in DT2 1.16 and DN2 1.11, `0x402b4a24`) rows used:
  `(0x11, "004GE0", enh 0x1d8, wp 1, erase 8, mlc 0x760000, slc 0x3b0000)` and
  `(0x70, "TX2932", enh 0x4db, wp 0x10, erase 1, mlc 0x3a4a000, slc
  0x2e1a000)`. **[D]**
- Why TX2932: the dialog ("MMC NOT IN SLC MODE", selected by
  `FUN_400337ba` when `FUN_4012da80()` is not 1) needs byte 0x98 = 1, and the
  check then compares capacity with the row's SLC constant. The 004GE0 SLC
  capacity (0x3b0000) is below the +Drive layout (content from 0x5ee180), so
  a 004GE0 card could not be both SLC and hold +Drive. The TX2932 row in pSLC
  mode (0x2e1a000) does. **[D]** for the reasoning.
- The 1.16 identity check accepts the default card: `DT2_SYX=Digitakt_II_OS1.16.syx
  uv run python -m pytest tests/test_esdhc_identity.py` passed. **[V]**
- `packages/emulator/card`, `packages/emulator/machine`, `packages/emulator/boot` (default and `diagnostics`),
  `packages/emulator/host`, `apps/desktop/src-tauri` (`cargo check` with and without
  `coupled-audio`) and the Python tests build and pass. Two mechanical fixes:
  `packages/emulator/card/tests/dma.rs` now expects SEC_COUNT bytes `02 e1 a0 00` for the
  default card, and one `mut` was dropped in `packages/emulator/card/tests/card.rs`. **[V]**

**Cold boot with the new default (native, ColdFire only).** **[V]**

- DN2 1.11, `--card-image` a +Drive image (`tools/firmware/plusdrive.py build samples`),
  `mise run boot`-style bounds: ready at 847,327,131 instructions, 106 s;
  completed main frame at 842,799,246 (sha256 `c7f7475b..`): LIGHTHOUSE main
  screen, no dialog. With `mlc4g` the same boot shows "MMC NOT IN SLC MODE".
- DT2 1.16, same image: ready at 914,826,692, 118 s; `fs_verification`
  success, completed frame sha256 `e6ee4f93..` (as in the 2026-10-01 seeded
  run). The first window is "FILE SYSTEM OK / PRESS Y/N TO CLOSE" as before;
  one NO press closes it and the main screen follows with no second dialog.
  With `mlc4g`, the first NO press shows "MMC NOT IN SLC MODE" (finding 03) and
  a second closes it.
- DN2 1.11, built-in empty +Drive card (`Emulator::new(syx, None)`, as
  `dspi2_capture` and `sharc_live` use): ready at 847,486,444; no dialog at
  ready+30M; NO presses change nothing. With `mlc4g` the dialog is present at
  ready+30M and the first NO closes it.
- No hang. The boot tool with no `--card-image` (blank card) does not reach
  ready within 1,000,000,000 instructions on DN2 1.11 (pc `0x401344b0`, main
  loop never started), with the new default, with `mlc4g`, and with a build of
  `HEAD` without the card change, so it is not caused by the identity. **[O]**
  Use a card image or the built-in empty card.

**New DN2 ready snapshot** `snapshots/dn2-audio-ready-2026-10-06/` (never
commit). Same files as the 2026-10-03 directory; the image is a copy, the
fresh DSP start state stays `snapshots/dn2-audio-ready-2026-10-03/digi-audio-loop2-fresh.bin`
(it cannot be rebuilt, keep it). **[V]**

    CF_SNAPSHOT=snapshots/dn2-audio-ready-2026-10-06/digi-audio-m5.snap CF_DIGEST=1 \
      out/native/target/prod/release/examples/sharc_live Digitone_II_OS1.11.syx \
      snapshots/dn2-audio-ready-2026-10-03/digi-audio-loop2-fresh.bin \
      snapshots/dn2-audio-ready-2026-10-06/digi-audio-dn2-image.bin out.wav 1000000

(example built with `SHARC_GEN_DIR=out/native/dn2-audio/gen cargo build --release
-p elektron-native-boot --features sharc --example sharc_live` into the prod
target dir). Wall time 5.5 s (6.9 s with the build lock): ready at
851,236,444 (was 851,236,347), DSP attached from the first frame, card
`slc32g`. A restore needs the same card: `coupled-run` refuses a snapshot whose
card capacity differs from the running card and names `DIGI_CARD_IDENTITY`. The
2026-10-03 snapshot needs `DIGI_CARD_IDENTITY=mlc4g`.

Checks with the new snapshot (`tools/emulator/digi_env.py` `READY` now points at it):

- `trig1-tap-release` without the NO prelude (Trig 1 down 60M, up 100M, 8.5 s):
  rms 0.1245, PCM sha256 `aa1dbd9e..`, the same hash as the standard scenario
  (with NO prelude) on the old snapshot. Control: the old snapshot with
  `mlc4g` and no prelude is silent (rms 0, `355ef6d0..`). **[V]**
- Standard scenarios, new snapshot: `exactness-100m` (1139 frames),
  `trig1-taps` and `trig1-tap-release` keep their `expect_sha256`;
  `trig1-noqa-40m` now sounds (rms 0.60) and its hash changed
  `a50586e8..` -> `680639a5..` (updated in `scenarios.json`). `idle`,
  `trig1-hold` and `kb-chord16` carry no pinned hash and ran clean. **[V]**
- Fine retrigger scan of finding 16 (f00..f24 at six DSP periods, 150 runs, the
  `taps/sweep.sh` + `an22.py` scratch scripts pointed at the new snapshot):
  0 bad, tone 94.3-95.2%. **[V]**
- `coupled_ready_exactness` (run as `tools/emulator/native_pgo.py` runs it, absolute
  `DIGI_COUPLED_FIXTURES`): 72,896 samples and PCM `3b600f6d..` unchanged. Two
  pins moved: CF digest `bc82b199..` -> `ab27c5c9ed6e..db23` (the card and
  DSPI RX state differ), DSP export `06980a01..` -> `c24a3040..00ff`. The old
  snapshot with `mlc4g` still reproduces `bc82b199..` and `06980a01..`, and
  the two DSP exports differ in 433 of 6,702,604 bytes. **[V]** for the
  numbers; that the 2026-10-03 snapshot predates the Type 17 fix, and that this
  explains the 433 bytes, is **[O]** (not traced).
- `mise run pgo` (263 s, 1139 frames in every training run), `mise run
  emu:build` and `tools/emulator/native_pgo.py verify` pass. **[V]**
- `desktop_trig1_capture_without_qa_script` with
  `DIGI_DESKTOP_TRIG1_REQUIRE_NONZERO=1` passes (it was silent before). **[V]**
- Finding 16, "Reproduce", says the ready snapshot has the SLC dialog open and
  a tap must not come first. That holds only for the 2026-10-03 snapshot **[C]**.

**[C]** 2026-10-07: the `snapshots/dn2-audio-ready-2026-10-06` DSP state is
superseded by a cold-start rebuild (`mise run emu:ready -- dn2`); the hashes pinned in
this section moved (see finding 16, "Reference state rebuilt from cold start").

## `--samples` launcher and the seeded-project hang (2026-10-07) **[D]**

Folded from a lane notebook; measured on DT2 1.16, not re-checked by a second agent.

- `mise run emu -- --cf-only --samples DIR SYX` (`tools/emulator/native_emu.py`) builds
  `packages/emulator/plusdrive`, runs `plusdrive build DIR -o img --syx SYX` and passes the image as
  `--card-image`. The image is cached in `out/plusdrive/native/<key12>/dt2.img`; the key is
  the SHA-256 of the firmware, the plusdrive binary and each sample name and bytes. Project
  seeding is DT2-only. `packages/emulator/boot/examples/card_boot.rs` boots a card headless
  (`ready_at=914986053`, `main_ui=true`, `fs_verified=Some(true)`, both sample names found
  in guest state).
- A card built with `--no-project` lists "Factory" and the samples in ASSIGN SAMPLE
  (FUNC+YES on the SRC page).
- **[C]** An earlier note blamed the seeded project for a UI hang on the SRC page. The
  cause was the model: it did not complete the DSPI1 command/response link. `FUN_400124a0`
  arms eDMA channel 14 (RX from DSPI1 POPR 0xfc03c03a). Its completion (INTC0 source 22,
  vector 86, handler 0x400ccd74) runs a callback; the peer then raises IRQ1 (EPORT, INTC0
  source 1, vector 65, handler 0x400ccd1c), and `FUN_40012500` reads 0x300 bytes back and
  gives semaphore 0x40965b84. Without both, the first `load_sample` never finished and held
  mutex 0x450d1b10, so the UI task blocked. Fix: `packages/emulator/machine/src/board.rs` retires the
  channel-14 major loop and zero-fills the destination; `packages/emulator/boot/src/runtime.rs`
  (`service_dspi1`) delivers vector 86, then vector 65. With it the seeded card calls
  `load_sample` 297 times and the SRC and FLTR pages draw. **[O]** The peer's identity;
  zero responses are enough.

## Track N plays file N: per-track project seeding (2026-10-12) **[D]**

`packages/emulator/plusdrive` (`project.rs`) no longer points all 297 slot references at the first file.
`point_refs_at(container, refs)` gives each of the 16 tracks of the active kit (kit 0) its own
table slot and its own file, `refs[t % n]` with the files in sorted-name order:

- A track keeps the slot it has if no lower track uses it. The built-in kit 0 shares slots (v3
  slots by track: 7, 1, 3, 7, 8, 9, 12, 1, 9, 4, 2, 2, 10, 6, 5, 11), and a shared slot has one
  reference, so a track whose slot is taken moves to the lowest unclaimed slot that already holds
  a reference (then to any free one). The slot int16 at `+0x3C + t*0x155 + 0x64` and the reference
  copy at `+0x129` are rewritten; `reset_active_kit` keeps both.
- The table slot of track t gets `refs[t % n]`; every other used slot and every other copy of an
  old reference in the container (other kits, tracks) becomes `refs[0]`. A kit other than kit 0
  therefore plays the first file wherever its slot was reassigned.
- The table still has 297 used entries (the claimed slots were all used already), so the firmware
  makes the same 297 `load_sample` calls. With one file the table equals the old seeding.

Checked with three generated 48 kHz mono files (220 Hz sine, 880 Hz sine, noise), card built in
process (`tools/emulator/card_check.py`, `packages/emulator/control/src/cards.rs`): the boot project's table slots and
track copies read back from the image as designed; at ready the DSP slot table (DM `0x257810`)
holds `{0x40, 0x18040, 0x30040}` for slots 7, 1, 3 and the SDRAM bytes at those starts are the
220 Hz sine, the 880 Hz sine and the noise (checked with `coupled-run --dump-dsp`); the UI shows
SAMP `A 7` for track 1 and `A 1` for track 2. **A panel TRIG n press plays track n, not the
selected track**: TRIG 1 after TRK+TRIG 2 still sounds track 1. With the TRIG n keys 25..27 the
captured PCM peaks at 222.7 Hz, 878.9 Hz and is broadband (peak share 0.02): track 1 plays file
1, track 2 file 2, track 3 file 3. **[O]** Whether a second agent reproduces this against the
image bytes; the bytes of kit 0 after the v3 -> v5 upgrade were not read back, only the UI and
the DSP.

### In-process card and ready boot **[D]**

`packages/emulator/control/src/cards.rs` builds the card (no Python); `packages/emulator/sharc-peer/src/cold.rs` is the
`sharc_live` cold boot (ColdFire plus the DSP from `cold-idle.dsp` on the DSPI2 link, snapshot at
ready once link port 0 has been quiet for 400 frames, `digi-audio-m5.snap` and `.snap.dsp`). The
controller's `card.load` runs both on a worker thread while the running guest waits, then
restores the pair and presses NO (button 12). Cached under `<run dir>/cards/<key12>/`; the key
hashes the firmware, the format tag `dt2-card-v2-track-n-file-n` and every file name and content.
Measured on this machine (loaded, load average about 5): three 1 s files, card build 1.2 s,
cold boot to ready 11.6 s (fast tier on; `tools/emulator/ready.py` takes about 2 min with the interpreter),
cached restore 0.3 s. **[O]** The in-app snapshot is made with the fast tier where `ready.py`
sets `SHARC_FAST_REGIONS=`; the exactness of the two ready states was not compared.

### Last card is mounted again at the next start **[D]**

After a successful `card.load` the controller writes `<run dir>/cards/last.json` (`key`, `name`,
`samples`, `firmware_sha256`; `name` is the chosen folder, `card.load {name}` or the directory name).
When the desktop app loads a DT2 firmware without `--card-image` (and without `--scenario`:
`Config::remember_card`), `last.json` names a card whose firmware hash matches, and the cached
image exists (with audio, the cached ready state too: a cold boot is never started at launch),
the controller mounts it before the first frame (the cached restore, about 0.3 s). `card.status`
then reports `state ready, auto true, name, samples`, and the toolbar shows "Samples: NAME (N
files)"; with nothing cached it shows "Choose samples to load a folder of WAVs". DN2 is
unchanged. Test: `packages/emulator/control/tests/card_load.rs`
(`the_last_card_is_mounted_again_when_the_dt2_starts`).

## `plus_drive/` folders: nested directories, plain files, extents, MmcFs slots (2026-10-16) **[D]**

`packages/emulator/plusdrive` (`tree.rs`) builds an image from a folder; the contract is in
`docs/SAMPLES.md`. What the builder relies on, read from the 1.16 decompile and confirmed by the
firmware's own calls (`tools/firmware/plusdrive_check.py`, `mise run plusdrive:check`, which boots the DT2
with a folder-built image and calls `FUN_40156fc4` / `FUN_40157072` / `FUN_401572e0` / `FUN_40158626` /
`FUN_4015832e`):

- **Directories of more than one page.** Entries never cross a 32 KiB content page and the last entry of each
  page has a slot that runs to the page end. The three index pages keep `page * 0x8000 + offset` with the
  directory's logical page. `FUN_401572e0` binary-searches `0x10000` by the unsigned name hash, then compares
  the names of equal keys; at 0xfff entries and more it falls back to a linear scan, and `FUN_40157072` only
  uses `0x10001` below 0xfff. The builder stops at 4,092 children (4,094 entries with "." and ".."). A
  702-entry directory (two content pages) lists and resolves correctly **[D]**.
- **Extents.** A record holds up to 8 inline 12-byte extents `{logical start, pages, physical page}`; with 9 or
  more it holds up to 24 page ids of extent-index pages, 0xaaa (2730) extents each
  (`FUN_40157d28`: `index = n / 0xaaa`, extent `n % 0xaaa`). The builder allocates files in one run, so a
  single extent is the normal case (a 64 MiB file is one extent); `--max-extent-pages N` splits runs into
  several extents to exercise the other forms. Files of 2 and 10 extents (indirect) read back equal through
  the firmware **[D]**.
- **Plain files** use the same record, content hash (`hash | 1` in record word +0x0c and the hash table) and
  directory entry as samples; the firmware does not look at the payload. A file named `factory` in the root
  is refused (the mount links the firmware's own `/factory`).
- **MmcFs slots.** Sector of slot `n`: projects `((n + 0xb) & 0xff) << 15` (16 MiB each, 128 slots; DT2
  `FUN_4012eef2`, DN2 1.11 `FUN_4012d7c2`), sounds `(n + 0x2400) * 8` on DT2 (4 KiB, 2048 slots) and
  `(n + 0x800) * 2` on DN2 1.11 (`FUN_4012de66`, 1 KiB), kits `0x16000 + n * 0x80` on DT2 (64 KiB, 1024
  slots) and `0x2000 + n * 0x40` on DN2 1.11 (`FUN_401308ac`, 32 KiB). The boot project at sectors 0x40000
  and 0x48000 is not a pool slot (it would be project slot 0xfd / 0xfe), so imported project slots cannot
  collide with it.
- The per-device numbers above (and the project regions at 0x40000, 0x48000, 0x50000, the path-filesystem superblock and the
  module store range) are data now: `[card]` in `devices/<device>.toml`, with the firmware functions cited there.
- **[C]** (finding 35) The bitmaps at sector 0x800 are write-protect bits, not occupancy; the builder no longer sets them. Earlier text, kept: The bitmaps at sector 0x800 are written as occupancy (bit `n & 7` of byte `n >> 3` of each pool's
  range: 16, 256, 128 bytes). `FUN_40130234`, `FUN_4013011e`, `FUN_40132c6c` set the bit from a flag argument
  and a per-pool slot test, and the kit and sound setters set it when their test returns 0, so the polarity
  of the sound and kit bits is not proven. No firmware call reads a folder-imported slot yet; the first slot
  files from a real exporter should be checked with `FUN_4012ecd8` (project) and the slot headers
  (`FUN_4012f6d8`). The DN2 addresses come from the 1.11 decompile only; 1.12 was not compared.
- **[O]** A directory's link count (record +2) is written as 2 plus the number of subdirectories; real cards
  were not compared.
