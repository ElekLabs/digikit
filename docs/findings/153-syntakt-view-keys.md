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

`[input.keys]` of the device file (name = the key's id in the firmware's key events, the one table of key names; `[input.key_flags]` names the flag bits) holds the key ids; core/ui takes YES,
LEVEL_PUSH (the second YES key), NO and RIGHT from it by name (`patchkit gen` refuses a device that names keys without one of them); `patchkit gen` writes `build/gen/ui-<dev>.inc`; a small
per-device object (`core/ui/src/keys/<dev>.s`) holds the four ids and the press and release flag bits, and `ui.c` reads them (`dk_ui_view_keys`). Digitakt II and Digitone II: 10, 0x31, 12, 15 (as before). Syntakt: 12, 48, 13, 17.
The ids were read from the decoder table of the stock image at 0x401d7a48 for the wire positions of YES, NO, RIGHT and the second YES key (panel codes 39, 28, 31 and 49: wire position = code - 1 = channel * 8 + bit); a patchkit test that compared them was removed (it tested data against firmware bytes, not behaviour).

## Data of the same kind, and what is not device data

- `[digikit] safe_key` (a firmware key index held at power-on) and `[input.keys]` (the key ids of key events) are the same kind of data: firmware key indices, which equal the panel code on the Digitakt II and Digitone II and
  differ on the Syntakt. They are kept apart because they are read by different code (the runtime's boot path, core/ui); unifying them is a data change for later.
- `[input.keys]` is required for a device whose patch sets include core/ui or core/digikit-menu (`patchkit gen` names the missing table).
- In an empty list `core/ui` swallows only YES and RIGHT (nothing to act on or open); every other key, including an unknown one, goes to the stock handler.
- The flag bits of a key event that the stock key handler tests are device data too (`press` and `release` of `[input.key_flags]`, generated into `ui-<dev>.inc`). Read from the Syntakt's compact
  menu view (`CompactVerticalMenuView` key handler 0x400f5c82) **[D]**: the accessor 0x400f97e4 (`flags & 1`) is used for the ids 15, 16 and 17 (the cursor keys and RIGHT: bit 0 moves) and 0x400f97b4
  (`(flags >> 4) & 1`) for the ids 12 and 0x30 (YES and the second YES key: bit 4 acts), the same bits as on the Digitakt II. That NO (13) acts on bit 4 too is **[O]** (NO leaves a page in the emulator).
- The second YES key of the Syntakt is id 0x30 = 48 (the LEVEL push, channel 6 bit 0 of the decoder table), read from the same handler **[D]**; the Digitakt II's is 0x31. It is now set in the device file.

## Open

- **[O]** The cause on the unit has not been confirmed by a flash; the emulator reproduction and the fix are the evidence.
- **[O]** The emulator run that first reached DIGIKIT in another analysis never opened the menu (YES on DIGIKIT); that run used another cursor position or timing. This one opened it.

- On the Syntakt the LEVEL push (id 48, `LEVEL_PUSH`) acts like YES in DIGIKIT lists, because core/ui treats LEVEL_PUSH as a second YES key; the stock handler treats id 0x30 like id 12 too, so this matches it. Em may revisit. A device without a LEVEL_PUSH key is refused by `patchkit gen` (the second YES key is required).
- **[O]** Device identity is the port-file prefix (`<short>-<version>`) in two places that predate this work: `targets_device` in `packages/patchkit/src/devgen.rs` and the gallery in `apps/web/src/data/modules.ts`. A module's `[targets]` keys and `ports` entries name devices only by that prefix; the port file format (devices/ports/) has no `device` field to read instead. Replace the prefix parse when the port file gets one.

