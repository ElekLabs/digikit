# Syntakt sample storage and the host path

Marks: **[V]** checked by a second agent in the decompiled MAIN of OS 1.42 (and in image bytes where stated), **[D]** read once, **[O]** open. Unmarked statements are **[D]**.
Image: `st-1.42` (`out/cfdb/st-1.42-emac.sqlite`). No emulator run. Claims marked "(observed on a real 1.42 unit)" were seen on a stock Syntakt. Context: [136](136-syntakt-peer-cpu.md) (peer CPU),
[138](138-syntakt-runtime-facts.md) (card layout), [141](141-syntakt-analog-and-nor-paths.md) (NOR), `docs/MIDI-SYSEX-RPC.md` (MidiRpc framing).

## Result

- Samples live on the eMMC, in sectors `0x2000..0x11fff`: 64 slots of 0x400 sectors (512 KiB). Not in a small storage area **[V]**.
- A slot holds a 0x40-byte header (magic `SAMP`, length in 16-bit samples at +8, name at +0xC) and then 16-bit mono samples.
  The largest data size is `0x7ffc0` bytes (262,112 samples, 5.46 s at 48 kHz). The manual says "5 seconds, approximately 0.5 MB".
- The peer CPU gets the full data of every slot, not 2 KB per slot. MAIN reads all 64 slots once at boot and streams them in 2 KB chunks.
  A new sample is therefore used only after a reboot (the manual says the same).
- The host path is the MidiRpc **Data API** (types `0x53..0x5e`) on the path `/samples/N`, N = 1..64 **[V]**; layouts, CRC and safety are in sections 4 and 6. It is not the FsSample (+Drive) API
  that the DT2 backend `stock-rpc` uses: MAIN 1.42 has no handler for the FsSample, FsRaw, StorageSpace and EnumerateFiles requests.
  `stock-rpc` cannot list, upload or delete Syntakt samples. A Data API client is needed.

## 1. Where and how: the slot record

Slot `s` (0-based, `s < 0x40`) starts at sector `((s + 8) & 0x1fff) << 10` = `0x2000 + s * 0x400`, byte offset `0x400000 + s * 0x80000`.
Slot 63 ends at sector `0x12000`. Evidence: `FUN_401132ec` (header validator, 0x401132ec), `FUN_4011149e` (table fill), `FUN_401133ce` (data read). **[V]** (offset formula, magic, length at +8 with the 0 and `0x7ffc0` limits, name loop of at most 16 characters at +0xc, data start at +0x40, header read from the first sector only).

Header (0x40 bytes, big-endian, read from the first sector of the slot):

| Offset | Field | Evidence |
| --- | --- | --- |
| +0x00 | magic `SAMP` (u32 `0x53414d50`) | `FUN_401132ec`: `*hdr == 0x53414d50`, else status -3 |
| +0x04 | not read by MAIN **[V]** | `FUN_401132ec`, `FUN_4011149e` and `FUN_401133ce` read only +0, +8 and +0xc.. |
| +0x08 | length in 16-bit samples (u32). Data bytes = length * 2 | `FUN_401132ec`: `hdr[2] * 2`; 0 gives -5, above `0x7ffc0` gives -6 |
| +0x0C | name, 16 bytes, NUL ended inside the 16 bytes | `FUN_4011149e` copies up to 0x10 chars from +0xC; rename `FUN_40111ed4` writes 0x10 bytes at +0xC |
| +0x1C..0x3F | not read by MAIN **[V]** (the host write stores them as sent: section 4) | |

Sample data starts at byte +0x40 of the slot (`FUN_401133ce`: `(offset + 0x40) >> 9` sectors, remainder `& 0x1ff`).

There is no sample rate, bit depth, channel count or checksum field that MAIN reads. The format is fixed by use: 16-bit, mono, 48 kHz
(manual 6.2.4; the peer converter runs at 48 kHz, finding 136). The 16-bit words are **big-endian, signed** (the ColdFire native order), end to end **[D]** (observed on a real 1.42 unit: after a restart a 1 s 440 Hz mono tone, written as big-endian signed 16-bit, played correctly). The static reading:
`FUN_401133ce` reads whole sectors (`FUN_4010f612`) and copies the slot bytes with the plain word copy `FUN_4011613c`;
`FUN_40082334` copies them again with `FUN_4011613c` into the upload buffer. No byte swap on the MAIN side. The peer (image `st-1.42-peer`, section 7)
indexes the buffer in `FUN_40000d06` (0x40000d06): per slot it assembles the 4-byte length word big-endian (`b0 << 24 | b1 << 16 | b2 << 8 | b3`)
and stores the table entry (length at `0x4404f754 + 8 * slot`, data pointer `0x4004f754 + offset` at `0x4404f758 + 8 * slot`). The voice setup
at 0x400097dc (not a Ghidra function; it reads that table at 0x40009972) hands the pointer to `FUN_40006764` (0x40006764), the sample reader:
it loads each sample with `mvs.w (0x0,A2,D5*0x2),D5` and `mvs.w (0x0,A2,A5*0x2),D7` (word load from the slot data, sign-extended, no `byterev`,
no `swap`, no byte loads), then interpolates linearly with a 12-bit fraction. Big-endian words in the slot are therefore played as written;
a little-endian WAV needs a byte swap on the host. The DT2 native sample is big-endian too, and the Syntakt header words are big-endian.
What Elektron Transfer writes into +0x04 and +0x1C..+0x3F is **[O]**. The firmware stores whatever the host sends there; a host tool should zero them.

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
- Whether the peer rejects or mishandles a bad slot is **[O]** (only its sample read path was read, see section 1).

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
The descriptor carries a callback `FUN_400b9c36` -> `FUN_4011149e` that refills one table entry; it runs at WriteClose and after Clear (`FUN_400b639a`) **[D]**. MAIN does not resend to the peer (boot only).

## 4. The host path: MidiRpc Data API

### Dispatch (the evidence that FsSample is not used)

`FUN_40108fbe` (0x40108fbe) is the RPC dispatcher (callers `FUN_40109d0c`, `FUN_40114dc6`; the latter is called from the `ObRpcCommunication` constructor at 0x4000a784). The only request classes whose type information has a
data reference are: Ping, SoftwareVersion, DeviceUID, Screenshot, Query, OsUpgrade (in `FUN_40108fbe`), Tempo (`FUN_4010c424`) and Data
(`FUN_4010bd9a`, 0x4010bd9a, called from `FUN_40108fbe` at 0x40109c3e) **[V]**. On the DT2 1.17 the same query shows 22 FsSample, StorageSpace and
EnumerateFiles requests in one dispatcher (`FUN_401286be`). On the Syntakt there is none. The message factory `FUN_4011ac50` (0x4011ac50)
still builds the FsSample and FsRaw classes (shared code), but nothing handles them **[D]**; what the device answers to such a request is **[O]**.
`refscan` was not run for this; a missing cast site in code outside functions is possible but the factory and the DT2 contrast make it unlikely.

### Frame and transport **[D]**

- The SysEx descriptor with device id `0x10`, separator `0x00`, command range `0..0x7f` exists in MAIN at 0x40242368 (jump table pointer `0x402d53b0`),
  the same record that the DT2 has (finding 42, `docs/MIDI-SYSEX-RPC.md` section 11). Two more records follow (ids `0x17`, `0x16`). The receive chain
  (8-in-7 unpack) was not traced on the Syntakt; it is shared Elektron code **[O]**.
- Unpacked request body: `seq_hi seq_lo len_hi len_lo type payload...`. `MidiRpcMessage::ctor` (0x4011d9c0) reads u16 BE at +0 (seq), u16 BE at +2 (length), u8 at +4 (type) and sets the read
  cursor to 5. Every field is read from the payload with big-endian helpers: `FUN_401b4e96`/`FUN_401b50fa` = u32 BE, `FUN_401b4fba` = u8, `FUN_401b5460` = string (bytes up to
  a NUL, at most 255 characters, NUL consumed). A missing byte reads as 0 (a u32 with fewer than 4 bytes left reads 0 and consumes nothing). **[V]**
  The length field is not used by the Data API parsers.
- Reply body: `counter_hi counter_lo seq_hi seq_lo (type|0x80) payload...`. The builder `MidiRpcMessage::ctor` (0x4011db1c) writes the u16 counter (global `0x444df470`, skips 0),
  then the request's seq. This is the DT2 layout (`docs/MIDI-SYSEX-RPC.md` section 11). Reply type bytes: List `0xd3`, ReadOpen `0xd4`, ReadPartial `0xd5`, ReadClose `0xd6`,
  WriteOpen `0xd7`, WritePartial `0xd8`, WriteClose `0xd9`, Clear `0xdc`. **[V]**
- Every Data reply starts with a status byte: 1 = OK, 0 = error. On error the rest is the error text, NUL ended (`FUN_401ad9c4` writes the characters and a final 0), and the
  WritePartial reply adds one more byte (a "fatal" flag) after the text. On OK the fields below follow. **[D]**
- The type byte of the factory (`FUN_4011ac50`, switch on `type - 1`): `0x53` List, `0x54` ReadOpen, `0x55` ReadPartial, `0x56` ReadClose, `0x57` WriteOpen, `0x58` WritePartial,
  `0x59` WriteClose, `0x5a` Move, `0x5b` Copy, `0x5c` Clear, `0x5d` Swap, `0x5e` Rename, `0x5f` SetTags. **[V]**

`RpcPort.rpc(type, payload)` of `packages/dk-session/src/rpcfiles.ts` takes a type byte and a payload and returns the reply payload (the bytes after the type byte). The Data API
needs exactly that, so the transport (`DkSession.rpc`: seq, 8-in-7 packing, reply match by seq) can be reused unchanged **[D]**. Three things differ in the client above it:
the first reply byte is the same status (1 = OK), but an error carries a NUL ended text instead of nothing; the path is a plain C string without a leading-slash rule
(`pathBytes` adds a `/` and a NUL: fine, the route needs `/samples/N`); and WriteOpen puts a u32 before the path. `rpcfiles.ts` `RpcFiles` itself is FsSample-only.
That the Syntakt answers on the same device id and framing is confirmed: a List request got a valid reply from a real 1.42 unit (observed on a real 1.42 unit).

### Request layouts (after the 5-byte header) **[V]**

All integers big-endian. `str` = bytes + NUL.

| Type | Request | Payload |
| --- | --- | --- |
| `0x53` | DataList (`FUN_4010a320`) | `str` path, u32 first index, u32 end index, u8 `exist_only` |
| `0x54` | DataReadOpen (`FUN_4010a7b8`) | `str` path, u32 chunk size (clamped to 0x10..0x4000), u8 `compress` |
| `0x55` | DataReadPartial (`0x4010bade`) | u32 job id, u32 sequence |
| `0x56` | DataReadClose (`0x4010aa2e`) | u32 job id |
| `0x57` | DataWriteOpen (`FUN_4010ab8a`) | **u32 declared size, then `str` path** |
| `0x58` | DataWritePartial (`FUN_4010b4de`) | u32 job id, u32 sequence, u32 CRC of the data, u32 data length, data |
| `0x59` | DataWriteClose (`0x4010b8e6`) | u32 job id, u32 (read by the constructor, not used by the handler) |
| `0x5c` | DataClear (`FUN_4010b1d4`) | `str` path |

The Data List and ReadOpen constructors are `0x401b58d2` and `0x401b96ca`; WriteOpen `0x401b980a`, WritePartial `0x401b61be`, ReadPartial `0x401b879c`, ReadClose `0x401b4f8e`,
WriteClose `0x401b894c`, Clear `0x401b89c4`. The earlier text that put the path first in WriteOpen was wrong: the u32 comes first. The declared size of WriteOpen is stored in the state
(`+0x30`) and no handler read of it was found; the writer size comes from the file header (below) **[D]**. Rename, Move, Copy, Swap and SetTags layouts: **[O]** (not needed for samples).
Paths are `/samples` (List) and `/samples/N`, N = 1..64, in decimal (section "Paths").

Sequence numbers: WriteOpen sets the expected write sequence to 0 and `WritePartial` must send 0, 1, 2, ... (a different value: "Invalid sequence number"). ReadOpen sets the read sequence to 0 and
`ReadPartial` must send 1, 2, 3, ...; the same value as the last one answered repeats the last chunk (retry), anything else is "Invalid sequence number". Job ids come from the global counter `0x402d5ac0`; a ReadOpen or
WriteOpen closes any job that is open (`FUN_4010a2a6`; state `+0x00`..`+0x20` read, `+0x24`..`+0x9c` write), so a client runs one job at a time. **[V]**

### Reply layouts (after the status byte 1) **[D]**

| Reply | Payload |
| --- | --- |
| List (`0xd3`, ctor `0x401aefa8`) | u32 start index (echo of the request), u32 next index (last returned index + 1; the request's end index when nothing was returned), u32 entry count, then the entries. Entry: `str` name (empty for an empty slot), u8 0, u8 kind (2 = item), u32 slot number 1..64, u32 size in bytes (0 = empty; else the audio bytes, `length * 2`, without the 0x40-byte header; observed on a real 1.42 unit: List returned all 64 slots, names and sizes matched the factory content), u16 permission word (`0x007e`), u8 exists (1 = used), u8 0 |
| ReadOpen (`0xd4`, `0x401af206`) | u32 job id, u32 chunk size (after the clamp), u8 `compress` (echo). **No total size.** |
| ReadPartial (`0xd5`, `0x401afa1e`) | u32 job id, u32 sequence, u32 progress (looks like permille; set only when the reader reports a total, else uninitialised: ignore), u8 complete (1 on the last chunk), u32 CRC of the data, u32 data length, data |
| ReadClose (`0xd6`, `0x401af31a`) | u32 job id, then a u32 that is stack garbage in the handler (ignore). Valid only when the reader is complete ("Reader did not complete") |
| WriteOpen (`0xd7`, `0x401af3c4`) | u32 job id |
| WritePartial (`0xd8`, `0x401af802`) | u32 job id, u32 sequence just accepted, u32 bytes accepted so far (headers, body and footer count) |
| WriteClose (`0xd9`, `0x401af974`) | u32 job id, u32 (`[O]`: the writer's statistics value, probably the bytes written into the slot; do not rely on it) |
| Clear (`0xdc`, `0x401af69e`) | no payload after the status byte |

List entries are 0x14 bytes in the handler (`FUN_400ba11c`: index, name, size, `0x7e`, exists). With first index 0 and end index 0 the list returns all 64 slots; `exist_only` = 1 drops empty
ones. A non-zero first or end index bounds the loop by slot number (`index >= end` stops). The list carries no hash or CRC and no flags other than the permission word and the exists byte.
Error texts seen: "File transfer is inactive", "File transfer is not active", "File transfer is complete", "No active transfer job", "Invalid job id", "Invalid sequence number",
"Invalid package checksum, corrupted data" (chunk CRC), "Unsupported payload size, header" (first chunk shorter than 0x1f), "Failed to write data", "Failed to read",
"Reader did not complete", "Footer was not processed", "Header was not processed", "Writer did not process all data", "Content hash mismatch", "Invalid file type",
"File is not compatible with this ...", "invalid sample slot", "sample slot out of range".

Chunk sizes. ReadPartial returns at most the chunk size of ReadOpen, which the firmware clamps to 0x4000 bytes; 0x10 at the least. WritePartial has no firmware maximum: the data length is a u32 of the request and the
firmware allocates a buffer of that size first (`FUN_4019ad2c`), so a client must send the real length. The bound is the SysEx message: finding 42 measured a gather limit of 0x20000 SysEx bytes on the DT2
(about 114 KB of payload); the Syntakt limit was not read **[O]**. A client should use 2048 bytes first (the `RPC_CHUNK` of `rpcfiles.ts`) and may raise it after one observation. The first WritePartial must hold
at least the 31-byte file header (0x1f bytes of data, else "Unsupported payload size, header"). The chunk CRC is the CRC below over the data bytes of the chunk. **[D]**

### The chunk CRC and the content hash **[V]**

Both use `FUN_4011e194` (0x4011e194) with the table `0x40247ef0` (raw bytes read from the image: entry 1 = `0x77073096`, entry 128 = `0xedb88320`; it equals the generated table of the reflected polynomial `0xedb88320`).
The function does `c = ~seed; for each byte: c = table[(c ^ b) & 0xff] ^ (c >> 8); return ~c`. The chunk CRC (`FUN_4011d9b4`, a wrapper that replaces the first argument by `-1`) therefore starts the register at 0.
Parameters: polynomial `0x04c11db7` (reflected `0xedb88320`), reflected input and output, **register init `0x00000000`, xorout `0xffffffff`**. This is the common CRC-32 with the seed `0xffffffff` instead of 0:
CRC of the ASCII string `123456789` is `0xd202d277` (the common CRC-32 gives `0xcbf43926`). Running form: `h = 0xffffffff; h = f(h, bytes)`, where `f` is the function above. The content hash
(`BackupFileImportAdapter+0xc`, start value `0xffffffff`) is the same function chained over the bytes that follow the 31-byte file header up to the end of the slot image. It does not cover the header or the footer. With
`compress` = 0 these are the slot image bytes. The CRC is big-endian on the wire (u32 fields).

### What a sample file is on this path

The item descriptor built by `FUN_400b9dfe` is a 0x7c-byte record: `+0x00` type 8 (sample), `+0x04` storage 2 (card), `+0x08` u16 permission word `0x7e`, `+0x0c` card byte offset `N * 0x80000 + 0x380000`
(= `0x400000 + (N - 1) * 0x80000`), `+0x10` size, `+0x14` id N, `+0x18` `0xffffffff`, `+0x4c` a callback that refills the slot table entry (`FUN_400b9c36` -> `FUN_4011149e`), `+0x6c` the file type check `FUN_400b9d7c`.
The size is `length * 2 + 0x40` (clamped to `4..0x80000`) for a used slot and 0 for an empty one (the List reply shows `length * 2`, section "Reply layouts"). Permission bits: bit 1 (`& 2`) read, bit 2 (`& 4`) write and rename, bit 3 (`& 8`) clear. **[V]**
Reads use `MmcStreamReader`, writes use `SafeMmcStreamWriter` (offset and size from the descriptor, ring buffer 0x10000).

The transferred bytes are a **backup file** (`BackupFileExportAdapter` 0x400b513a, `BackupFileImportAdapter` 0x400b50e6, import `vfunc_2` 0x400b535c). Layout, all big-endian:

| Offset | Size | Field | Import check |
| --- | --- | --- | --- |
| 0x00 | 4 | magic `0xac11d303` | none |
| 0x04 | 1 | version 2 | none |
| 0x05 | 2 | `0x0008` | must be 8 (`FUN_400b57e4`, "File is not compatible with this ...") |
| 0x07 | 2 | `0x000d` | none |
| 0x09 | 4 | a word of the firmware version record (`*(FUN_4011621c() + 8)`) | none **[O]**: value not decoded |
| 0x0d | 4 | type 8 (sample) | must be 8 (`FUN_400b9d7c`, "Invalid file type") |
| 0x11 | 4 | `0xffffffff` | none |
| 0x15 | 4 | id: slot number 1..64 | none (the slot comes from the path) |
| 0x19 | 4 | size in bytes of the slot image | **sets the writer size, no clamp** (see section 6) |
| 0x1d | 1 | compress flag | 0 = raw; non-zero wraps the writer in `Lz4StreamDecompressor` |
| 0x1e | 1 | `0x0c` (footer length) | none |

After the header (31 bytes) comes the slot image (`size` bytes), then the footer (12 bytes): u32 CRC of the slot image bytes (checked: "Content hash mismatch"), u32 0 (not checked), u32 `0xaaa1daaa` (not checked).
Total file length = 31 + size + 12. Bytes after the footer are ignored. **[D]** (the export constructor sets the values; the import reads only the checks in the table and the footer CRC; `FUN_400b4fde` and `FUN_400b5090` copy header and footer for the checks).

LZ4 is optional: the flag at +0x1d (export: the `compress` byte of ReadOpen) selects it, and a client that always sends 0 never needs LZ4. The LZ4 framing is **[O]**, not needed. **[D]**

### The write path end to end **[D]**

1. WriteOpen: resolves the path (`FUN_400b5764`, so a bad slot fails here), stores the path, the declared size and a new job id; no data is written.
2. First WritePartial (sequence 0): needs at least 0x1f data bytes. The firmware resolves the path again, copies the descriptor, **replaces the descriptor size by the u32 at +0x19 of the data** and creates the writer
   (`FUN_400b6204` -> `FUN_400b60b6`: `SafeMmcStreamWriter(offset, size, 0x10000)`). Then it hands the chunk to `BackupFileImportAdapter::vfunc_2`: the first 31 bytes fill the header buffer, the rest goes to the writer
   (and to the content hash). **The header checks (+5 and +0xd) run after that, so the body bytes of the first chunk are already in the writer when a bad header is refused** **[D]**.
3. The writer erases the first 512-byte sector of the slot at its first write (`FUN_4010f52a`), buffers the first sector in RAM, writes the rest sector-wise and writes the first sector last, when the last byte of `size` has arrived
   (`SafeMmcStreamWriter::vfunc_2`). A slot is therefore empty (no `SAMP`) from the first write until the last body byte, and valid from that moment, **before the footer CRC is checked**. A last partial sector is read, merged and written back.
4. The bytes in the slot are the image bytes as sent: the 0x40-byte header (`SAMP`, +4, length at +8, name at +0xc, +0x1c..0x3f) and the samples. **The firmware builds nothing in the slot**; it does not insert a name, a
   length or a magic. MAIN reads only `SAMP`, +8 and +0xc (section 1), so bytes +4 and +0x1c..0x3f are stored as sent and never read.
5. Further chunks continue the adapter (`vfunc_2` 0x400b535c). It calls the body writer once per chunk and does not loop: bytes the body writer does not take go to the footer buffer (12 bytes, `+0x4c`, buffer `+0x40`), but bytes the writer takes are body, counted by the writer and hashed. When the footer buffer is full the handler (`FUN_4010b4de`, after every chunk) calls `FUN_400b5090` and then `FUN_400b58e6`, which compares the CRC (error "Content hash mismatch") and sets "footer processed" (`+0x26`). **The footer must start in a WritePartial that comes after the call that completes the body writer. A chunk that holds the end of the body and the footer together loses the footer** (rule below) **[D]** for the counter reading; the failure itself was observed on a real 1.42 unit (error "Footer was not processed", first sector left erased, slot read "NO SAMPLE" after a restart).
6. WriteClose: needs header and footer processed and the writer complete ("Writer did not process all data"). It then calls the descriptor callback: `FUN_4011149e` reads the first sector of the slot from the card and
   refills the RAM table entry (name and length). The peer still gets samples only at boot (section 2).

The firmware does not check that the u32 length at +8 of the sample header matches `size`; the boot reader uses the length at +8 only.

### Why "Footer was not processed": the writer accepts more than `size` bytes **[D]** (static reading of the counters; the refusal was observed on a real 1.42 unit)

Answers for the body/footer switch:

1. The body size is exactly the u32 at +0x19 (no rounding, no +-0x40): `FUN_4010b4de` copies it to the descriptor `+0x10` (`local_f4 = *(data+0x19)`), `SafeMmcStreamWriter::ctor_dtor` (0x400bbfa4) takes it as `size`. The WriteOpen size, +0x09 and the SAMP length word are not used. The writer is built from a 0x200-byte first-sector RAM buffer (`+0x230`) and an `MmcStreamWriter` (`+0x250`, ctor 0x400bbb32) for the remaining `size - 0x200` bytes at slot offset + 0x200. For slot 63 the card offset is `63 * 0x80000 + 0x380000` = 0x2300000.
2. The footer is collected by a `MemoryStreamWriter` of capacity 12 (adapter ctor 0x400b50e6), counted across chunks correctly (`MemoryStreamWriter::vfunc_2` 0x400bb45e clamps to the free space), and needs no WritePartial of its own **if the body writer stops taking bytes at `size`**. It does not.
3. `MmcStreamWriter::vfunc_2` (0x400bbbe4, disasm at 0x400bbbf6: `move.l (0x28,A2),D1; cmp.l (0x2c,A2),D1; bls`) clamps one call to `min(length, max(+0x28, +0x2c))`. `+0x2c` is the bytes still to accept, `+0x28` the bytes still to write to the card, and `+0x28 >= +0x2c` always, so the clamp is `+0x28`: the writer accepts `+0x28 - +0x2c` bytes beyond the end, namely all bytes still in its ring buffer (full sectors are flushed after each call, so that is the size's sector remainder: 481 bytes here). Those extra bytes are the footer. `+0x2c` wraps below 0, so the loop never reaches its tail flush (`+0x2c == 0 && +0x28 < 0x200`), `MmcStreamWriter::vfunc_3` stays false, `+0x28` never reaches 0, and the first sector is never written. The adapter sees the body writer take the whole chunk, passes nothing to the footer buffer, `FUN_400b5090` reports "not full", no error is returned, and WriteClose (0x4010b8e6) tests `+0x26` first: "Footer was not processed". The footer CRC and the two other footer words are not involved (a CRC mismatch would give "Content hash mismatch" at the chunk).
4. Our file (31 + 96064 + 12 = 96107 bytes, 2048-byte chunks): chunk 0 has 31 header and 2017 body bytes; chunks 1..45 are body; chunk 46 has 1899 bytes = 1887 body + 12 footer. Before chunk 46 the writer had accepted 93665 bytes after the first sector and flushed 93184 (481 in the ring); `max(2368, 1887)` allows all 1899. Result: the first sector was erased (`FUN_4010f52a` at the first write) and not rewritten (it stays in RAM and is dropped); slot bytes 0x200 .. 0x200 + 95232 were written (image bytes 512..95744); the last 320 image bytes and the 12 footer bytes stayed in the ring and were dropped. Slot 63 therefore has no `SAMP` and reads as empty; the table callback did not run.

Client fix: end a WritePartial exactly at offset `31 + size` (the end of the slot image) and send the 12 footer bytes as their own WritePartial (or later). The header values stay as they are (+0x19 = 96064; file length 96107). A last body chunk that ends exactly at `size` is accepted whole (`+0x2c` = 0), the writer completes (tail sector read, merged and written, then the first sector), the next chunk of 12 bytes fills the footer buffer, the CRC matches and WriteClose passes. Splitting chunks only so that the footer starts in a new chunk is not enough if the body-end chunk also carries footer bytes. Chunks that are full body bytes before the end are safe. **[D]** for the counters. Observed on a real 1.42 unit: the same file sent with the body ending on a chunk boundary and the footer as its own 12-byte chunk was accepted, a read-back matched the bytes sent, and the sample played correctly after a restart.

### The existing backend

`packages/dk-session/src/rpcfiles.ts` and the `stock-rpc` backend of `samples.ts` send FsSample commands (`0x10`, `0x20`, `0x40`..`0x42`, `0x28`) to
a path filesystem with folders, names and a native file with a 0x40-byte header (stereo flag, length at +4, rate at +8) and a 16-byte trailer.
None of this fits the Syntakt: no handler (section 4), a different slot header (length at +8, name at +0xc, no rate) and no folders. `RpcFiles` is not reusable, but its `RpcPort` transport is
(see "Frame and transport"). A Syntakt backend needs a Data API client on `RpcPort.rpc` (the layouts above), the slot model (64 numbered slots, a List per refresh), the backup file wrapper with the CRC above, and the
converter output (mono 16-bit 48 kHz, at most 262,112 samples, header with name). The content of header bytes +4 and +0x1c..0x3f that Transfer writes stays **[O]**.

## 5. How a track picks a sample

The Twinshot machine has two sample parameters, SMP1 and SMP2, that select a sample slot; a negative value plays the same slot in reverse (manual,
Twinshot page; strings "Sample 1" and "Sample 2" at `0x402545c9`, `0x402545ec`). The saved identity is the slot number 1..64: the manual says the samples
are global and that removing, replacing or rearranging samples changes every project that uses them. So a project refers to a slot, not to a name or a
content hash. The mapping from the parameter value to the slot and the other machines that use slots are **[O]**.

## 6. Safety: what a host tool must never write

Verdict **[V]**: the Data API write is **not safe against a bad host header**. Nothing between WriteOpen and the eMMC write limits the write to the slot.

Trace of every bound, in order:

1. Route: `FUN_400b9dfe` accepts N = 1..64 only (`N - 1 < 0x40`, unsigned, so 0 and negative values fail; "invalid sample slot", "sample slot out of range"). The offset is computed from the same N. A slot number
   outside 1..64 cannot reach a write on `/samples/*`. The path is resolved at WriteOpen and again at the first WritePartial. The other two routes (`/projects`, `/soundbanks`) have their own descriptors and writers: a sample client must never use them.
2. Size: `FUN_4010b4de` stores `*(u32*)(data + 0x19)` into the descriptor size (`+0x10`) with no comparison. The WriteOpen declared size is unused. The sample descriptor's own size (the existing sample length) is replaced.
3. Writer: `SafeMmcStreamWriter` (0x400bbfa4) and `MmcStreamWriter` (0x400bbbe4) accept at most `size` bytes in total (`min(remaining, chunk)`) from the offset on, upward, with no upper limit tied to the slot.
4. eMMC: `FUN_4010f7f8` (write) and `FUN_4010f52a` (erase) check only that the sector is below the card size (`_DAT_444ba720`, error -10 / -13) and that the count is not negative. The MmcFs guard (refuse sector `>= 0x180000`) is not on this path.

So a header with `size` above 0x80000 followed by more than 0x80000 bytes writes into the next slots and, from slot 64, into `0x12000` upward, up to the end of the card (the project pool at `0x80000..0x17ffff`, the COKi copies at `0x7a000`
and `0x7c000`, the digikit store from `0x800000` on the Syntakt layout) **[D]**. The writer cannot write below the slot start and cannot start at any other sector than the slot's. The bytes that follow `size + 0xc` are ignored.

Rules for a client:

- Set +0x19 to `0x40 + 2 * length`, at most `0x7ffc0 + 0x40 = 0x80000`, and send exactly `31 + size + 12` bytes. Check `length * 2 <= 0x7ffc0` and a slot 1..64 on the host before sending.
- Send header type 8 and u16 8 at +5, a valid CRC, and the real data length in every WritePartial.
- Write only paths `/samples/N`. The firmware starts erasing the slot at the first write; an aborted upload leaves an empty slot (no `SAMP`) and does not touch the neighbours.
- Everything else on the card belongs to the firmware: sector 0 (MmcFs header), 0x800 (write-protect bitmaps), `0x1000..0x1fff` (2048 sound slots),
  `0x7a000`, `0x7c000` (COKi copies), `0x7e000..0x7ffff` (working project), `0x80000..0x17ffff` (128 projects). See finding 138.
- The firmware's own erase of all samples touches `0x2000..0x11fff` only. A reboot is needed after any slot change before the sound engine sees it.
- A digikit store path for samples (as on the DN2) would put the data in the store at `0x800000+` and not in the slots. The firmware would not play it:
  the machine reads the slots only. An adapter must therefore write the slots (or hook the boot upload `FUN_40082334`).

## 7. Open items

- **[O]** Syntakt SysEx gather limit (largest safe WritePartial). One observation: WritePartial with 4096, 16384 and 32768 data bytes into a scratch slot.
- **[O]** Header bytes +0x09, and what Elektron Transfer writes in the sample header at +4 and +0x1c..+0x3f. One observation: ReadOpen of one sample and the bytes of the reply.
- **[O]** The second u32 of the WriteClose reply, the third u32 of ReadPartial and the trailing u32 of ReadClose (stack values; a client ignores them).
- **[O]** LZ4 framing (not needed with `compress` = 0).
- **[O]** What the peer does with a slot whose length is 0 or large (peer code not read), the panel rename path, Move/Copy/Swap internals.
