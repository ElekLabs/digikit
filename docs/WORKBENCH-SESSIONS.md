# Workbench sessions

A session is an emulator project. It keeps the card, firmware choice and a
collection of captures or listening files together. A capture is a recording
you can still open on its own, with no firmware, card or MIDI connection.

The **Session** menu sits beside the firmware picker in the Emulator toolbar.
Switching to a physical device hides the menu without closing the session.

## Start with the card you already have

1. Run `mise run wb`. Open firmware if it does not auto-load.
2. Wait until the emulator is ready and has mounted its card. Save the
   pattern or project **on the emulated instrument** if you changed it.
3. Choose **Session → Save As…**. The initial name is **Untitled Session**.
   The picker suggests a new folder under `~/Music/Digikit Sessions/` when
   available. You can put the folder elsewhere.
4. Choose **Start emulator** in the Session menu when you want to switch
   from the normal card to the private copy. Saving the copy does not stop
   playback or change the original card. **Not mounted** means the session is
   open for inspection, and the currently running emulator, if any, is separate.

If there is no mounted card, choose **New…**, then **Use card image…** or
**Build from +Drive folder…**. A folder is imported once, not watched or
mirrored. An imported `.img` is read on its own. Save the live card if you
also want the guest writes that might sit beside that image.

## Reopen, save and fork

- **Open…** selects the folder's `session.json`. It is safe to open it
  without the emulator or firmware. **Recent sessions** is only a local list
  of folder paths.
- **Start emulator** boots a separate working card. For moved firmware,
  choose **Locate firmware…** and select the same file. Its SHA-256 must
  match. Once a working card has been saved, the loaded image hash is pinned
  too. A changed checkout build is refused instead of silently substituted.
- **Save** writes the working card's overlay as a checkpoint and then
  replaces the document. If a save fails before the document is published,
  the previous checkpoint remains the one that opens. Save does not capture
  firmware RAM. If a pattern was edited, save it on the instrument first.
- **Reload firmware** and the emulator's **Reset** keep the working card.
  **Revert to last save** deliberately replaces it with the saved one.
  **Close** discards uncheckpointed working changes after confirmation.
  Session cards do not export or mirror into the normal +Drive folder.
- **Save As…** creates a new session with an independent card base and
  checkpoint. If already working on a session card, later saves go to the
  copy. Older captures and WAVs stay with the original session.

The File menu provides New, Open, Save and Save As shortcuts while the
Emulator pane is active. Workbench keeps the session itself in its folder,
not in app preferences.

## Captures and the CLI

Use **Attach file…** to copy an existing capture, WAV or audio report into
the session. Open an attached capture from the Session menu to see it in
Profile and Performance. Workbench's **Save capture…** now suggests
`*.capture.json` for its existing `dk.profile/1` format. **Open capture…**
still reads older `*.profile.json` and `*.dkprof.json` files and calibration
capture sets. Workbench recordings made on a mounted session include the
session ID and the saved checkpoint at the start of recording. This
identifies the baseline, and does not imply the live card contained no
unsaved changes.

Copy or move the entire folder to keep its relative card and attachment
paths intact. Firmware stays outside it. The card base is sparse, so use a
sparse-aware copy when moving it between volumes. Do not commit the bundle
because it may contain samples, built modules and exported data.

```sh
mise run wb:cli -- --format json session inspect "$HOME/Music/Digikit Sessions/Untitled Session"
mise run wb:cli -- --format json profile report /path/to/recording.capture.json
```

The first command returns the session document and resolved asset paths,
including the checkpoint and any attachments. The second accepts a capture
from that list or a standalone recording. Neither command starts the
emulator or connects to the device.

For the Underpass fixture, import `underpass-138.img`, attach the existing
Underpass capture, and start the session on matching firmware. After boot,
load saved Project 001, pattern A01. A session restores the card, not the
device's last screen or working RAM.

The [DT2 stock Delay reference](INSTRUMENTATION.md#replay-the-dt2-stock-delay-reference)
can generate a new session and private probe card directly from the CLI.
