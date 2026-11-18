# Syntakt sample storage and the host path

All claims are **[D]** (read once in the decompiled MAIN of OS 1.42, the cfdb and the manual) unless marked **[O]** (open). Nothing here is **[V]**.
Image: `st-1.42` (`out/cfdb/st-1.42-emac.sqlite`). No emulator run, no device run. Context: [136](136-syntakt-peer-cpu.md) (peer CPU),
[138](138-syntakt-runtime-facts.md) (card layout), [141](141-syntakt-analog-and-nor-paths.md) (NOR), `docs/MIDI-SYSEX-RPC.md` (MidiRpc framing).

## Result

- Samples live on the eMMC, in sectors `0x2000..0x11fff`: 64 slots of 0x400 sectors (512 KiB). Not in a small storage area.
- A slot holds a 0x40-byte header (magic `SAMP`, length in 16-bit samples at +8, name at +0xC) and then 16-bit mono samples.
  The largest data size is `0x7ffc0` bytes (262,112 samples, 5.46 s at 48 kHz). The manual says "5 seconds, approximately 0.5 MB".
- The peer CPU gets the full data of every slot, not 2 KB per slot. MAIN reads all 64 slots once at boot and streams them in 2 KB chunks.
  A new sample is therefore used only after a reboot (the manual says the same).
- The host path is the MidiRpc **Data API** (types `0x53..0x5e`) on the path `/samples/N`, N = 1..64. It is not the FsSample (+Drive) API
  that the DT2 backend `stock-rpc` uses: MAIN 1.42 has no handler for the FsSample, FsRaw, StorageSpace and EnumerateFiles requests.
  `stock-rpc` cannot list, upload or delete Syntakt samples. A Data API client is needed.

## 1. Where and how: the slot record

Slot `s` (0-based, `s < 0x40`) starts at sector `((s + 8) & 0x1fff) << 10` = `0x2000 + s * 0x400`, byte offset `0x400000 + s * 0x80000`.
Slot 63 ends at sector `0x12000`. Evidence: `FUN_401132ec` (header validator, 0x401132ec), `FUN_4011149e` (table fill), `FUN_401133ce` (data read).

Header (0x40 bytes, big-endian, read from the first sector of the slot):

| Offset | Field | Evidence |
| --- | --- | --- |
| +0x00 | magic `SAMP` (u32 `0x53414d50`) | `FUN_401132ec`: `*hdr == 0x53414d50`, else status -3 |
| +0x04 | not read by MAIN **[O]** | no code reads it |
| +0x08 | length in 16-bit samples (u32). Data bytes = length * 2 | `FUN_401132ec`: `hdr[2] * 2`; 0 gives -5, above `0x7ffc0` gives -6 |
| +0x0C | name, 16 bytes, NUL ended inside the 16 bytes | `FUN_4011149e` copies up to 0x10 chars from +0xC; rename `FUN_40111ed4` writes 0x10 bytes at +0xC |
| +0x1C..0x3F | not read by MAIN **[O]** | |

Sample data starts at byte +0x40 of the slot (`FUN_401133ce`: `(offset + 0x40) >> 9` sectors, remainder `& 0x1ff`).

There is no sample rate, bit depth, channel count or checksum field that MAIN reads. The format is fixed by use: 16-bit, mono, 48 kHz
(manual 6.2.4; the peer converter runs at 48 kHz, finding 136). The byte order of the 16-bit words is **[O]**: MAIN copies the words
unchanged to the peer, and the peer code was not read. The DT2 native sample is big-endian, and the Syntakt header words are big-endian too.
What Elektron Transfer writes into +0x04 and +0x1C..+0x3F is **[O]**. A header written by a host tool should zero them.

MAIN keeps a RAM table of the 64 slots in the `MmcFs` object: entry `s` at `this + 0x12e28 + s * 0x18` = 0x14 bytes of name (NUL ended) and
the length word at `+0x14`. A length of 0 means empty. `FUN_401112f4` (is slot used), `FUN_40111434` (size = length * 2 + 0x40),
`FUN_401113b6` (name). The table is filled by `FUN_4011149e` for each slot (`FUN_401115ea`, 64 calls). A fill that does not find `SAMP`
or finds a length above `0x7ffc0` leaves the entry empty. Callbacks registered on the object (`FUN_40111216`) run after each fill.

## 2. Limits and what the peer receives

- Data: at most `0x7ffc0` bytes per slot, header included at most `0x80000` = one slot. Checked at boot by `FUN_401132ec` and at table fill by
  `FUN_4011149e`. 64 slots x 0.5 MiB = 32 MiB: the manual's "32 MB".
- Mono only (the length is in 16-bit words; there is no channel field). Stereo is not supported by this reader.
- Boot upload, `FUN_40082334` (0x40082334, called once from the boot thread `FUN_40082578`, then `FUN_40082e9c` starts the peer thread):
  1. buffer at `0x4d84f2c0` (`FUN_40082a48`); byte 0 = `0xAA`, bytes 1..4 = total payload length, big-endian;
  2. for each slot 0..63: `FUN_401133ce(slot, hdr, tmp, 0, 0x80000)`; on error 4 zero bytes; else 4 bytes (the header length word, copied from +8)
     and `length * 2` data bytes. Empty and invalid slots cost 4 bytes each.
  3. `FUN_40082f90` (0x40082f90) sends the buffer in 2 KB chunks (0x800 bytes) through the FlexBus window `0x10000800`, sequence number at `0x1000000c`.
     The 2 KB is the chunk size, not a per-slot size. Finding 136 states the same; its "peer stores at 0x4004f750 + seq * 0x800" is the chunk position.
  The buffer ends at `0x4f84f2c0`, where the peer's section 7 buffer (`0x4f84f2c1`) starts: the 32 MiB `0x4d84f2c0..0x4f84f2c0` hold at most
  `5 + 64 * (4 + 0x7ffc0)` bytes. The progress bar uses `FUN_400829f4`.
- No decimation, no truncation, no conversion. The full length goes to the peer.
- Whether the peer rejects or mishandles a slot is **[O]** (peer code not read here).

## 3. Readers and writers

Readers:

| Who | Chain |
| --- | --- |
| Boot, header table | `FUN_4000a890` (0x4000a984) -> `FUN_4011160c` -> `FUN_401115ea` -> `FUN_4011149e` x 64 -> `FUN_401100d4` -> eMMC read `FUN_4010f612` |
| Boot, data to the peer | `FUN_40082578` -> `FUN_40082334` -> `FUN_401133ce` x 64 -> `FUN_401132ec` (validate) -> `FUN_4010f612` |
| Any sample present | `FUN_4003301a` -> `FUN_40111358` -> `FUN_4011337c` x 64 (**[O]** what uses the answer) |
| Host read (Data API ReadOpen) | section 4, stream `MmcStreamReader` over the slot |

Writers and deleters:

| Action | Chain | Writes |
| --- | --- | --- |
| Host write | Data API WriteOpen/WritePartial -> `FUN_4010ab8a`, `FUN_4010b4de` -> `FUN_400b6204` -> `FUN_400b60b6` -> `SafeMmcStreamWriter` | the slot bytes, sector by sector; the first sector last (see below) |
| Host delete (Data API Clear) | `FUN_4010b1d4` -> `FUN_400b639a` -> `FUN_4010f52a` | erase of the first sector (the header) only; needs permission bit 8 |
| Rename (Data API Rename; a panel rename path is **[O]**) | `FUN_4010aeaa` -> `FUN_400b5b3e` (type 8) -> `FUN_40111ed4` -> `FUN_40111628` | the name bytes in the header sector (read-modify-write of 0x200 bytes at +0xC) |
| Erase all samples (the manual's settings tick box "SAMPLES"; the link to this function is **[O]**) | `FUN_40058238` (0x40058304) -> `FUN_40112052` | erases sectors `0x2000..0x11fff` in chunks of 0x800 (`FUN_4011015e` -> `FUN_4010f52a`), then refills the table |

`SafeMmcStreamWriter::vfunc_2` (0x400bc0ce) erases the first sector of the target when it starts, writes the rest, and writes the first
sector (the header) last. An interrupted write leaves no `SAMP` magic, so the slot reads as empty. The writer stops at the end of its range
(error "Attempting to write to a completed stream").

Move, Copy and Swap (`FUN_4010acec`, `FUN_4010b016`, `FUN_4010b320`) exist for slots (the manual: rearranging samples); their internals are **[O]**.
The descriptor carries a callback `FUN_400b9c36` -> `FUN_4011149e` that refills one table entry **[O]** (when it runs). MAIN does not resend to the peer (boot only).

## 4. The host path: MidiRpc Data API

### Dispatch (the evidence that FsSample is not used)

`FUN_40108fbe` (0x40108fbe) is the RPC dispatcher (callers `FUN_40109d0c`, `FUN_40114dc6`). The only request classes whose type information has a
data reference are: Ping, SoftwareVersion, DeviceUID, Screenshot, Query, OsUpgrade (in `FUN_40108fbe`), Tempo (`FUN_4010c424`) and Data
(`FUN_4010bd9a`, 0x4010bd9a, called from `FUN_40108fbe` at 0x40109c3e). On the DT2 1.17 the same query shows 22 FsSample, StorageSpace and
EnumerateFiles requests in one dispatcher (`FUN_401286be`). On the Syntakt there is none. The message factory `FUN_4011ac50` (0x4011ac50)
still builds the FsSample and FsRaw classes (shared code), but nothing handles them **[D]**; what the device answers to such a request is **[O]**.
`refscan` was not run for this; a missing cast site in code outside functions is possible but the factory and the DT2 contrast make it unlikely.

Type bytes of the Data API (factory switch `FUN_4011ac50`, reply type = type | 0x80, framing as in `docs/MIDI-SYSEX-RPC.md`):

| Type | Request | Handler | Fields (wire order, from the request constructors) |
| --- | --- | --- | --- |
| `0x53` | DataList | `FUN_4010a320` | path string, u32 first index, u32 end index, u8 flag (1 = existing entries only) |
| `0x54` | DataReadOpen | `FUN_4010a7b8` | path string, u32 chunk size (clamped 0x10..0x4000), u8 compress flag |
| `0x55` | DataReadPartial | direct reply | job id, sequence number (**[O]** exact layout) |
| `0x56` | DataReadClose | direct reply | |
| `0x57` | DataWriteOpen | `FUN_4010ab8a` | path string, u32 declared size |
| `0x58` | DataWritePartial | `FUN_4010b4de` | u32 job id, u32 sequence (from 0, +1 per chunk), u32 CRC of the payload, u32 length, payload bytes |
| `0x59` | DataWriteClose | direct reply | |
| `0x5a` / `0x5b` / `0x5c` / `0x5d` / `0x5e` / `0x5f` | Move / Copy / Clear / Swap / Rename / SetTags | `FUN_4010acec` / `FUN_4010b016` / `FUN_4010b1d4` / `FUN_4010b320` / `FUN_4010aeaa` / none | Clear: path string. Rename: **[O]** |

Errors seen as strings in the handlers: "File transfer is inactive", "Invalid job id", "Invalid sequence number", "Invalid package checksum",
"Unsupported payload size, header" (first chunk shorter than 0x1f bytes), "Failed to write data". The job counter is the global `0x402d5ac0`.
The DT2 knows the same Data API for projects and soundbanks (finding 25 and `docs/MIDI-SYSEX-RPC.md` section 12); no client of it exists in
`packages/dk-session`.

### Paths

`FUN_4000776e` (0x4000776e) builds the route registry at `0x405d4580` with three handlers: `ProjectHandler`, `SoundbankHandler`,
`SampleHandler` (0x40235f7c; the DT2 has two). The RPC state object keeps the registry at `+0xc0` (`FUN_4010a22c`). `SampleHandler::vfunc_3` (0x400ba304)
registers two routes: `/samples` (list, `FUN_400ba270` -> `FUN_400ba11c`) and `/samples/*` (item, `FUN_400ba0d8` -> `FUN_400b9dfe`).
`FUN_400b9dfe` parses the last path segment as a decimal number with `FUN_40132964` ("invalid sample slot" when it is not a number; "sample slot
out of range" unless 1 <= N <= 64). Slot index = N - 1. So the path is `/samples/1` .. `/samples/64`; the name of a sample is not part of the path.

`/samples` lists the 64 slots: for each, index 1..64, the slot name (`FUN_401113b6`; empty for an empty slot), the size in bytes (0 for empty, otherwise
`length * 2 + 0x40`, at least 4, at most 0x80000) and the permission word `0x7e`, plus an "exists" byte (`FUN_401112f4`). `FUN_400ba11c`.

### What a sample file is on this path

The item descriptor built by `FUN_400b9dfe` has: type 8 (sample), storage 2 (card), permission word `0x7e`, card byte offset
`N * 0x80000 + 0x380000`, size (see above), id N. Permission bits (read in the handlers): bit 1 (`& 2`) read, bit 2 (`& 4`) write and rename,
bit 3 (`& 8`) clear. Reads use `MmcStreamReader`, writes use `SafeMmcStreamWriter` (offset and size taken from the descriptor, buffer 0x10000).

The transferred bytes are a **backup file**, not the raw slot (`BackupFileExportAdapter` 0x400b513a, `BackupFileImportAdapter` 0x400b50e6 and
`vfunc_2` 0x400b535c):

- 31-byte header (export constructor): +0x00 u32 `0xac11d303`, +0x04 u8 2 (version), +0x05 u16 8, +0x07 u16 13, +0x09 u32 firmware-related word
  (`*(FUN_4011621c() + 8)`), +0x0D u32 type (8), +0x11 u32 `0xffffffff`, +0x15 u32 id (slot number 1..64), +0x19 u32 size in bytes of the slot
  image, +0x1D u8 compress flag, +0x1E u8 12 (length of the footer).
- the slot image: the 0x40-byte `SAMP` header and the data. With the compress flag set, an LZ4 stream (`Lz4StreamCompressor` /
  `Lz4StreamDecompressor`, class names in the cfdb) **[O]** (framing not read).
- 12-byte footer: three u32; the first is a content hash that `FUN_400b58e6` compares with the running hash ("Content hash mismatch"); the third is
  `0xaaa1daaa` on export. A CRC32 style table at `0x40247ef0` (`FUN_4011e194`) is used for the content hash and the chunk CRC (`FUN_4011d9b4`).
  Seed, final xor and which polynomial: **[O]**.

The write validator `FUN_400b57e4` needs the u16 at +5 to be 8 ("File is not compatible with this ..."); the sample callback `FUN_400b9d7c` needs the
u32 at +0x0D to be 8 ("Invalid file type"). `FUN_4010b4de` takes the u32 at +0x19 of the first chunk and stores it as the size in the descriptor copy
before the writer is built. **No clamp to 0x80000 was seen** in `FUN_4010b4de`, `FUN_400b6204` or `FUN_400b60b6`. The magic `SAMP` and the length
are not checked by the Data path: a bad header gives a slot that the boot code ignores, and an oversize value lets the writer run beyond the slot **[D]**.

Not decided statically (**[O]**): the exact reply layouts of DataList and DataReadOpen/WriteOpen, the DataReadPartial request layout, the LZ4 framing,
the CRC parameters and what Elektron Transfer puts into the header words and the unread header bytes. One observation settles all of them: a capture
of the MidiRpc messages of one Transfer upload of one sample to an emulated Syntakt (or the sector bytes of the slot afterwards plus the capture).

### The existing backend

`packages/dk-session/src/rpcfiles.ts` and the `stock-rpc` backend of `samples.ts` send FsSample commands (`0x10`, `0x20`, `0x40`..`0x42`, `0x28`) to
a path filesystem with folders, names and a native file with a 0x40-byte header (stereo flag, length at +4, rate at +8) and a 16-byte trailer.
None of this fits the Syntakt: no handler (section 4), a different slot header (length at +8, name at +0xC, no rate) and no folders. The backend
is not reusable without change. A variant would replace the commands (Data API), the path model (64 numbered slots), the file model (backup file
with header, CRC and footer) and the converter output (mono 16-bit 48 kHz, at most 262,112 samples, header with name).

## 5. How a track picks a sample

The Twinshot machine has two sample parameters, SMP1 and SMP2, that select a sample slot; a negative value plays the same slot in reverse (manual,
Twinshot page; strings "Sample 1" and "Sample 2" at `0x402545c9`, `0x402545ec`). The saved identity is the slot number 1..64: the manual says the samples
are global and that removing, replacing or rearranging samples changes every project that uses them. So a project refers to a slot, not to a name or a
content hash. The mapping from the parameter value to the slot and the other machines that use slots are **[O]**.

## 6. Safety: what a host tool must never write

- Samples: only sectors `0x2000..0x11fff` and only through slot images of exactly one slot (at most 0x80000 bytes, `SAMP` header first).
  Never trust a size field from a file: keep it at or below 0x80000 and write nothing past the slot.
- Everything else on the card belongs to the firmware: sector 0 (MmcFs header), 0x800 (write-protect bitmaps), `0x1000..0x1fff` (2048 sound slots),
  `0x7a000`, `0x7c000` (COKi copies), `0x7e000..0x7ffff` (working project), `0x80000..0x17ffff` (128 projects). See finding 138.
- The MmcFs write guard (refuse sector `>= 0x180000`) does **not** apply to the Data API writers: `MmcStreamWriter::vfunc_2` calls `FUN_4010f7f8` directly.
  Only the descriptor (offset from the slot number, size from the host header) bounds them. The digikit store (`devices/syntakt.toml [card.store]`,
  from sector `0x800000`) is far above the slots, so a slot overflow of up to 512 KiB cannot reach it, but a larger value could.
- The firmware's own erase of all samples touches `0x2000..0x11fff` only. A reboot is needed after any slot change before the sound engine sees it.
- A digikit store path for samples (as on the DN2) would put the data in the store at `0x800000+` and not in the slots. The firmware would not play it:
  the machine reads the slots only. An adapter must therefore write the slots (or hook the boot upload `FUN_40082334`).
