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

`[input.view_keys]` of the device file (`yes`, `yes2` (0: none), `no`, `right`) holds the key ids; `patchkit gen` writes `build/gen/ui-<dev>.inc`; a small object per device (`core/ui/src/keys/<dev>.s`) holds the four
words and `ui.c` reads them (`dk_ui_view_keys`). Digitakt II and Digitone II: 10, 0x31, 12, 15 (as before). Syntakt: 12, 0, 13, 17. The patchkit test
`the_syntakt_view_keys_are_the_decoder_table_entries` reads the table from the stock image and compares.

## Data of the same kind, and what is not device data

- `[digikit] safe_key` (a firmware key index held at power-on) and `[input.view_keys]` (the key ids of key events) are the same kind of data: firmware key indices, which equal the panel code on the Digitakt II and Digitone II and
  differ on the Syntakt. They are kept apart because they are read by different code (the runtime's boot path, core/ui); unifying them is a data change for later.
- `[input.view_keys]` is required for a device whose patch sets include core/ui or core/digikit-menu (`patchkit gen` names the missing table).
- In an empty list `core/ui` swallows only YES and RIGHT (nothing to act on or open); every other key, including an unknown one, goes to the stock handler.
- The flag bits of a key event that the handler tests (bit 0 for RIGHT, bit 4 for YES and NO) are named `KEYFL_MOVE` and `KEYFL_ACT` in `ui.c`. They are the Digitakt II's bits (finding 71); on the Syntakt bit 4 is seen to
  work for NO and YES (the emulator run above), bit 0 for RIGHT is **[O]**. They stay in code until a device differs.

## Open

- **[O]** The cause on the unit has not been confirmed by a flash; the emulator reproduction and the fix are the evidence.
- **[O]** What `yes2` (0x31 on the other devices) is. The Syntakt has no key with that id in its table; it stays 0.
- **[O]** The emulator run that first reached DIGIKIT in another analysis never opened the menu (YES on DIGIKIT); that run used another cursor position or timing. This one opened it.
