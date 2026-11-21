# Filter descriptor rows and the knob requests

DT2 OS 1.17 and DN2 OS 1.12. Static reading of MAIN OS bytes and the Ghidra dumps for the machines service (plan E7a). Companion of [148](148-filter-machines.md). Marks: **[D]** read once, **[O]** open (the first emulator iteration decides).

## The descriptor row

- The filter accessor (`cf.filter.fn.accessor`) returns a row of 0x2c bytes: eleven longs. Words 0 and 1 are the long and short name (string characters, as the machine rows), words 2 to 10 are nine parameter codes (the registry calls them `f0` to `f8`). **[D]**
- The firmware's initialiser fills the six rows. Type 0 (Multi-Mode) has codes `0x2d 0x2e 0x2f 0x30` in f0 to f3, `0x24 0x25 0x26` in f4 to f6, `0x31` in f7 and 10 in f8 (the track level), on both devices (DT2 write at 0x401c4e4e..0x401c4eb4, DN2 at 0x400caffa..0x400cb03c). The FLTR1 page shows f4 to f6 as its three type knobs. **[D]**
- The parameter row of each code is in the 60-byte parameter table (`cf.param.data.descriptor_table`): word 0 the owner, word 1 the mirror index of the value in the sound, words 2 to 4 minimum, maximum and default (raw, unit * 256), word 8 the code. **[D]**

| code | role | DT2 mirror | DN2 mirror |
| --- | --- | --- | --- |
| 0x24 | Multi-Mode f4 (the cutoff: its default is 0x7f00, flag word 1) | 0x24 | 0x43 |
| 0x25 | Multi-Mode f5 | 0x25 | 0x44 |
| 0x26 | Multi-Mode f6 | 0x23 | 0x42 |

  Every type has its own three codes (type 1 has 0x27 and on), and all of them use these three mirror words: the three knob words of the sound are shared by the filter types. **[D]**

## The knob requests

- The function `FUN(code, machine, filter)` (DT2 `FUN_400da3a2`, DN2 `FUN_400dc18e`) answers the parameter code of a mirror index. A request in the three consecutive mirror indexes (DT2 0x23 to 0x25, DN2 0x42 to 0x44) reads the RAM table `3 * type + (request - first)` (DT2 0x4316ea8c, DN2 0x42c65cd0) for a type up to 5 and answers 0 above. So the request is the mirror index and the answer is the code of the type's knob that has it. The table is RAM and its fill was not found, so which field a request reaches is read from the mirror indexes above, not from the table. **[O]** for the fill, **[D]** for the rest.
- A module filter takes the knob field of the stock knob that has the same mirror index: field 4 is the stock cutoff code (read from the stock row at set-up, so no code is written in the service), fields 5 and 6 are run-time parameter rows made from the stock row of the same field as template, with its mirror index.

## Other sites

- The order vector function (DT2 `FUN_40052c98`, DN2 `FUN_4004dae2`) keeps a static vector of six longs and, on every call, allocates a new buffer, copies the static vector into it and fills the object at a0 (begin, end, capacity). The builder walks that copy and frees its begin with the firmware free after the loop (DT2 0x400611e2 calling 0x4011eb5a, DN2 0x4005b552 calling 0x401204ce). The copy is the caller's, so the service replaces it with a longer one and frees the stock buffer; it never touches the static vector. The builder reads neither d0 nor a0 after the call. **[D]**
- Whether the firmware's initialiser fills the stock rows before the services are applied is not established from the image; the service completes its rows at the first tick after the stock row holds its words. **[O]**
- The chooser's list builder (DT2 `FUN_400610e8`, DN2 `FUN_4005b458`) calls the order vector function with `jsr abs.l` at +0x86 (a0 = the vector object it fills), then walks begin to end. The target of that call can be replaced. **[D]**
- The loader's filter byte: DT2 0x400db484..0x400db4a6 and DN2 0x400dd420..0x400dd442 are 34 bytes with no branch (the bound 7 is the immediate of the first instruction, the store is the last). The record byte is rec[0xd3] on DT2 and rec[0xf5] on DN2; the sound byte +0xa3 and +0xdf. **[D]**
- The graph switch needs no hook for a type of 6 or more: it already takes the default branch (no curve). **[V]** in 148.
