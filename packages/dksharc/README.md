# dksharc

Static analysis of the SHARC+ DSP section of a digikit module. Rust, no Python; builds for the host and `wasm32-unknown-unknown`, so the
workbench can use it in the browser.

```rust
let input = dksharc::Input::from_dkds(dkds_section_bytes, dko_json_bytes)?;
let analysis = dksharc::analyze(&input);
let json = serde_json::to_string(&analysis)?;
```

`Input` is a plain struct (text parcels, relocations, imports, symbols, entries); `from_dkds` fills it from the DKDS section of a `.dkm`
(layout in `packages/patchkit/src/dkmdsp.rs`) and the `.dko.json` of the DSP object (text size and symbols). A caller that has the pieces
already can fill the struct itself.

## What it gives (`Analysis`, serde JSON)

- `insns`: every confidently decoded instruction: pc (short word), length in parcels, form name, import name of a relocation on it, memory operands.
- `functions`: named from the `.dko`. They start at the first instruction, at global text symbols, at the voice and effect entries of the
  DKDS section and at the targets of calls inside the module. Local labels are not functions.
- `blocks` of each function, with the control transfer that ends the block, its delay slots (`delay_slots`, two instructions after a delayed
  transfer) and edges (`fall`, `taken`, `loop_back`, `return_site`).
- `loops`: DO loops with the pc of the DO, first and last body instruction, count (`lit`, `reg` with the Ureg, `until` for a condition),
  parent and depth.
- `calls`: inside the module (`local`), import calls resolved through a class 2 relocation on the call (`import`, label `import:<symbol>`),
  indirect (`indirect`) and calls to a direct address outside the text (`outside`).
- memory operands per instruction at form level: DM or PM, load or store, mode (post-modify, pre-modify, immediate offset, direct address),
  I and M register numbers (I0-I15, M0-M15), immediate or address, normal, long word or sub-word access.

## Fail closed

An instruction that does not decode as `Confident` is listed in `undecoded` (pc, `unknown` or `uncertain`, the raw parcels) and is not
part of `insns`. The sweep goes on after it, by the decoded length for an uncertain form and by one parcel for an unknown word. The functions
that contain it have `complete: false`, as do functions with a control transfer that is not modelled (`10a_rel`, RTI, loop re-entry).
Structural findings (a branch into a delay slot, a DO whose end is not an instruction, a symbol off an instruction start) are in
`diagnostics`.

## Limits

- Linear sweep: data in the text section (tables) decodes as code. Symbol kinds could mask them (not done).
- Delayed-branch slots are the next two decoded instructions. No data flow: register def/use is slice 2, from a table generated from
  `tools/sharc_dev/sharc_core` (instruction semantics stay there).
- No width override from an assembler listing: decoding uses `sharc_rt::decode::decode_at` (lookahead for the 16-bit and 32-bit width ties).
- Memory operands name index registers, not addresses: no value tracking.

## Tests

`cargo test -p dksharc` or `pnpm nx:pilot run dksharc:rust-tests`. `tests/pluck.rs` is the oracle on our pluck machine for the
Digitone II (`tests/fixtures/`: the DKDS section and the DSP `.dko.json`; no firmware bytes). `tests/synthetic.rs` has nested loops, delay
slots, calls, an import call, an unknown word and an uncertain decode.
