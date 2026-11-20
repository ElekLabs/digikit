# Syntakt: DIGIKIT > MODULES froze the device (core/ui used Digitakt II key ids)

Date: 2026-11-20. Image: `st-1.42` MAIN. Context: [138](138-syntakt-runtime-facts.md) "Safe key", [137](137-syntakt-patch-dry-run.md). Marks: **[V]** checked against image bytes, **[D]** read or measured once, **[O]** open.

## Result

On a Syntakt, DIGIKIT > MODULES showed an empty list ("NO STORE") and no key worked afterwards, not even NO: the device had to be power cycled.

1. **Cause [D].** `core/ui` builds a list page (`DK_UI_LIST`) as the firmware's `MenuWithPopoutSideBarsView` and replaces its key handler. The handler tells YES, NO and RIGHT by the id of the key
   event (the word at +0xc of the event). It had the Digitakt II ids as constants: YES 10, NO 12, RIGHT 15. An empty list (no rows: the store is missing, or empty) lets only NO through
   to the stock handler and swallows every other key. On the Syntakt the id 12 is **YES**, and NO is **13**, so NO was swallowed with the rest.
2. **The Syntakt ids [V].** The firmware's key decoder table at 0x401d7a48 of st-1.42 holds one word per wire position (channel * 8 + bit): the firmware key index. Channel 3, bit 3 (panel code 28, NO)
   is 13; channel 4, bit 6 (code 39, YES) is 12; channel 3, bit 6 (code 31, RIGHT) is 17. The Digitakt II and Digitone II tables are the identity (finding 138 "Safe key"), which is where 10, 12 and 15 come from.
3. **Reproduction in the emulator [D].** `panel_drive` on the Syntakt `device` image (panel codes 25 SETTINGS, 30 DOWN, 39 YES, 28 NO; the cursor on DIGIKIT after about 20 DOWN): open MODULES; the page's row
   counter `DK_LOAD_N` is 1 ("NO STORE"); clear it, press NO and YES: it stayed 0, the page never opened again. With the ids from the device file the counter is 1 again after NO and YES. The pages
   ABOUT and MEMORY (text pages, stock key handler) re-open as before in both runs. The same happened on a blank card, so it is not the store state.

## What was built

`[input.view_keys]` of the device file (`yes`, `yes2` (optional; absent: none, generated as 0xffffffff), `no`, `right`, `move_flag`, `act_flag`) holds the key ids; `patchkit gen` writes `build/gen/ui-<dev>.inc`; a small object per device (`core/ui/src/keys/<dev>.s`) holds the six
words and `ui.c` reads them (`dk_ui_view_keys`). Digitakt II and Digitone II: 10, 0x31, 12, 15 (as before). Syntakt: 12, 48, 13, 17. The patchkit test
`the_syntakt_view_keys_are_the_decoder_table_entries` reads the table from the stock image and compares.

## Data of the same kind, and what is not device data

- `[digikit] safe_key` (a firmware key index held at power-on) and `[input.view_keys]` (the key ids of key events) are the same kind of data: firmware key indices, which equal the panel code on the Digitakt II and Digitone II and
  differ on the Syntakt. They are kept apart because they are read by different code (the runtime's boot path, core/ui); unifying them is a data change for later.
- `[input.view_keys]` is required for a device whose patch sets include core/ui or core/digikit-menu (`patchkit gen` names the missing table).
- In an empty list `core/ui` swallows only YES and RIGHT (nothing to act on or open); every other key, including an unknown one, goes to the stock handler.
- The flag bits of a key event that the stock key handler tests are device data too (`move_flag`, `act_flag` of `[input.view_keys]`, generated into `ui-<dev>.inc`). Read from the Syntakt's compact
  menu view (`CompactVerticalMenuView` key handler 0x400f5c82) **[D]**: the accessor 0x400f97e4 (`flags & 1`) is used for the ids 15, 16 and 17 (the cursor keys and RIGHT: bit 0 moves) and 0x400f97b4
  (`(flags >> 4) & 1`) for the ids 12 and 0x30 (YES and the second YES key: bit 4 acts), the same bits as on the Digitakt II. That NO (13) acts on bit 4 too is **[O]** (NO leaves a page in the emulator).
- The second YES key of the Syntakt is id 0x30 = 48 (the LEVEL push, channel 6 bit 0 of the decoder table), read from the same handler **[D]**; the Digitakt II's is 0x31. It is now set in the device file.

## Open

- **[O]** The cause on the unit has not been confirmed by a flash; the emulator reproduction and the fix are the evidence.
- **[O]** The emulator run that first reached DIGIKIT in another analysis never opened the menu (YES on DIGIKIT); that run used another cursor position or timing. This one opened it.

- On the Syntakt the LEVEL push (id 48, `yes2`) acts like YES in DIGIKIT lists, because core/ui treats `yes2` as a second YES key; the stock handler treats id 0x30 like id 12 too, so this matches it. Em may revisit. A device with no second YES key leaves `yes2` out of `[input.view_keys]`; it is then generated as 0xffffffff, which no key event has.
- **[O]** Device identity is the port-file prefix (`<short>-<version>`) in two places that predate this work: `targets_device` in `packages/patchkit/src/devgen.rs` and the gallery in `apps/web/src/data/modules.ts`. A module's `[targets]` keys and `ports` entries name devices only by that prefix; the port file format (devices/ports/) has no `device` field to read instead. Replace the prefix parse when the port file gets one.
- **[D]** `UI_NO_KEY` (0xffffffff, what `yes2` is generated as when absent) is no key id. Syntakt 1.42: the button decoder 0x40103094 takes the id of an event from the table at 0x401d7a48, which has 49 words with values 0 to 0x30 (read from the image); the only -1 in that function is the internal "last key" variable (0x402d531c), which is not copied into an event. Digitakt II 1.17 and Digitone II 1.12: the decoders (0x4011e318, 0x4011fc8c) use identity tables over the panel's wire positions (finding 58), so the ids are the positions, below 0x31 + 1 for the panels of both devices. I did not re-decompile the Digitakt II and Digitone II decoders for this note: the claim for them rests on finding 58 and the table being the identity.

