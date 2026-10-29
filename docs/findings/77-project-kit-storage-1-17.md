# Project and kit storage on DT2 1.17 and DN2 1.12

Marks: **[D]** read or measured once, **[O]** open, **[C]** corrects an earlier claim. Nothing here is **[V]**: a
second agent has not checked it against the image bytes. Measured with the scripted runner
`packages/emulator/coupled/examples/zero_patched.rs` ([INSTRUMENTATION](../INSTRUMENTATION.md), "Scripted patched
runs") on patched builds that hook the routines below, and on the stock files. RAM tables on 1.16:
[finding 09](09-runtime-state.md). Project slots on the card: [finding 14](14-plus-drive-format.md).

## 1. The project in RAM **[D]**

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Project object (what the autosave passes to the save routine) | `0x41776c9c` | `0x41219970` |
| Patterns: + `0x1c`, 128 records | `0x41776cb8`, stride `0x1db8c` | `0x4121998c`, stride `0x1de6e` |
| Kits: project + `0xedc61c` / + `0xef371c` | `0x426532b8`, 128 of `0x5574` | `0x4210d08c`, 129 of `0x5d71` |

Each pattern has its own kit: kit n is pattern n's. Both OSes also have a kit library (KIT MANAGER: SAVE KIT, LOAD KIT
TO PATTERN, PASTE KIT, RELOAD KIT FROM SAVED).

The pattern on is `*(*PATTERN_OBJ + 0x138)`, with `PATTERN_OBJ` DT2 `0x44f370a0`, DN2 `0x446488d0` (the pointer the
getter DT2 `0x40198b7e` / DN2 `0x4018ac5a` returns). It was the only word that went 0, 1, 2 as A01, A02 and A03 came
on (16 MB RAM dumps). A write guard on it during a pattern change reported one writer, DT2 `0x401a239c`.

A RAM record has no spare room. A DT2 kit's `0x5574` bytes are all serialized. A DT2 pattern record's bytes the save
leaves out, + `0x1cef0` .. `0x1db60`, are in use: `0xff`, and a 16 x 128 array the code at `0x400d6cd6` reads.
Pokes there hung the UI.

## 2. The saved project **[D]**

A project file starts with the mark `0xbeefbace` and version 5. The name is at + 8 (16 bytes) and a word at + 24.
128 pattern records of `0x15c00` follow from offset 512, then the kit records from `0xae0200`. The file ends with
`0xbacef00c`: at `0xdd9600` on DT2 (file `0xdd9604` bytes), at `0xc4b000` on DN2. The header's + `0x1c` .. `0x200`
is never read or written: the save and load routines touch only + 0, 4, 8 and 24, and the records from 512.

| | DT2 1.17 | DN2 1.12 |
| --- | --- | --- |
| Save routine `(buffer, project, ...)` | `0x400df66c` | `0x400e15f8` |
| Load routine | `0x400df95a` | `0x400e18e6` |
| Pattern serializer / loader | `0x400dc894` / `0x400dc702` | `0x400de90c` / `0x400de77a` |
| Kit serializer / loader | `0x400dbdca` / `0x400db898` | `0x400ddfa8` / `0x400dddb6` |

Pattern record, DT2: version 4, then 16 tracks x `0x4a0` (from RAM `i * 0x6a5`), `0xc000` (from RAM + `0x6a50`),
`80 x 258` (from RAM + `0x17e50`), 0x28 bytes raw from RAM + `0x1db60`, ending at `0x15acc`. On DN2 the tracks are
`0x4a3` long and the raw tail is 0x12a bytes from RAM + `0x1dd40`, ending at `0x15bfe`. The DT2 loader's version
converters (`0x400de220` from 3, `0x400de420` from 2) show the raw tail moved from `0x15a94` to `0x15aa4` between
versions.

Kit record: the mark `0xbeefbace`, version 4, then the fields, and the end mark `0xbacef00c` at DT2 + `0x573a` / DN2 +
`0x28ab`. Records are `0x5800` (DT2) / `0x2a00` (DN2) apart.

Bytes no serializer writes (zero in every save seen):

| | Unused |
| --- | --- |
| DT2 pattern record | `0x15acc` .. `0x15c00` (308 bytes) |
| DT2 kit record | `0x573e` .. `0x5800` (194) |
| DN2 pattern record | `0x15bfe` .. `0x15c00` (2) |
| DN2 kit record | `0x28af` .. `0x2a00` (337) |
| Header (both) | `0x1c` .. `0x200` (484) |

### Places to hook **[D]**

The same instructions on both OSes; each is 6 bytes, one whole instruction:

| | DT2 | DN2 | Registers there |
| --- | --- | --- | --- |
| Kit save: `move.l #0xbacef00c,d1`, before the end mark | `0x400dbee4` | `0x400de0d4` | a2 the record, a3 the kit's RAM |
| Kit load: the same instruction, after the version check | `0x400db8be` | `0x400ddddc` | DT2: a2 the kit's RAM, a3 the record; DN2: the other way round |
| Header save: `move.l #0xbeefbace,(a3)` | `0x400df932` | `0x400e18be` | a3 the file; d0 is live after it |
| Header load: `move.l #0xbeefbace,d0` | `0x400df99c` | `0x400e1928` | a3 the file |
| DT2 pattern save: the serializer's memcpy of the raw tail | `0x400dc8cc` | | d2 the record, d3 the pattern |
| DT2 pattern load: `move.l #0x1db88,d0` | `0x400dc7e6` | | a2 the pattern, d2 the record + `0x10a04` |

Besides the project save and load, the kit serializer has nine other callers on each OS: DT2 `0x40035292`,
`0x40035532`, `0x40035760`, `0x40035790`, `0x40043834`, `0x400d3bae`, `0x400e2ee8`, `0x400e3034` and
`0x401332b8`. The kit loader has four: DT2 `0x40044612`, `0x400485be`, `0x40048978` and `0x400e1570`.
- `0x40044612` loads into the table: kit n of the project object, from a copy at `0x409908e4` (the reload).
- `0x40043834` saves a table kit into a buffer at `0x40996ce4`.
- The others pass kit pointers that were not traced **[O]**.

In a save each kit record passes the serializer twice, and in a load the loader twice. A hook counting per pass sees
two **[D]**.

### The save buffer **[D]**

The DT2 buffer at `0x4099d698` holds the file loaded at boot, and ends where the project object begins. SAVE PROJECT
AS right after boot does not write from it: bytes poked into it did not reach the card, and the new file has zeros
in the unused bytes. A save that follows another save serializes into the buffer the loaded file is in, so the bytes
no serializer writes keep what the loaded file had. Seen on both OSes, on the stock file alone: a project with
non-zero unused bytes, loaded and saved as another slot, kept them.

### What a load and a save change **[D]**

On the stock OS alone, a project saved, loaded and saved again changes only the header word at + 24, which the load
copies to the project's RAM + 8 and the save writes back.

## 3. Copies of a kit in RAM **[D]**

Pattern copy and paste (FUNC + RECORD, FUNC + STOP) copies the kit with memcpy (DT2 `0x40136388`, DN2 `0x40134770`)
of the kit's size. Seven calls on each OS push that size as memcpy's length (DT2 `0x40035e3e`, `0x40035ee0`,
`0x40036c30`, `0x40133412`, `0x401a900a`, `0x401a9080`, `0x401cdcdc`); others pass it in a register. memcpy begins with
three loads of its arguments, 12 bytes of whole instructions:
`movea.l 4(sp),a1; movea.l 8(sp),a0; move.l 12(sp),d0`.

Measured: with a hook there, the paste of A01 onto A04 reached A04's kit through kit-sized memcpy calls only:
either straight from A01's kit, or from a buffer an earlier kit-sized copy had filled from it (the hook followed
both). PASTE KIT and undo were not driven **[O]**.

## Open **[O]**

- Which caller of the kit serializer and loader is which kit manager action, and whether all pass table kits.
- Whether any copy of a kit goes another way than memcpy (a loop, part by part).
- The DN2 1.12 pattern serializer's other callers.
