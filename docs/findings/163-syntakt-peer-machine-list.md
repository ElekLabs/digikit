# Syntakt 1.42 peer machine selection

The opt-in peer audio adapter can offer one machine in the native list on digital audio tracks. Double-tap SYN to open the list, use the arrows to select the row, and press YES to bind the peer voice. NO restores the previous stock preview. Choosing a stock row clears the peer binding on that track. [Peer note dispatch](162-syntakt-peer-note-dispatch.md)

## Firmware seams

- **[V]** The initial selection call at `0x40031052` takes a built view and a stock type. The stock constructor at `0x4002f868` builds its rows into a `Menu` at view offset `0x4c`. Digital tracks have spare vector capacity on 1.42. The adapter adds a borrowed resident `MenuItem` only when a slot is free. Stock fetch and teardown skip reference counting when the shared pointer's control word is zero. Insertion uses no heap allocations. [Firmware declarations](../../ports/fw/machine.toml)
- **[V]** The native arrow and encoder path calls `0x40031242` with a signed delta. Stock movement clamps against the catalog and updates a stock preview type. The adapter counts stock rows past the cursor, skipping dividers. At the peer row it moves the native highlight and retains the stock preview type for returning to stock. The row's ID stays inside the menu.
- **[V]** The close routine at `0x4002f346` takes a view and a confirm flag. Stock confirm takes the machine type from view offset `0x1cc`. Peer confirmation queues a binding and takes the stock cancel path to restore the opening machine. Stock confirmation checks the latest requested binding, including one awaiting a peer acknowledgment. When the mailbox is busy, the list stays open so stock confirmation cannot race a pending peer bind.

## Emulator proof

Follow the [adapter recipe](../../runtime/platform/peer-audio/README.md) with `samples/next` on the card. The fixture offers the `SHOT` label and stages the sample. Panel confirmation assigns it to track 1. Check both mailbox words at `0x100007f4` after preview, cancel, confirm, the first note, stock confirm and the next note. Expect `0,0`, `0,0`, `1,0`, `1,1`, `0,1` and `0,1` respectively. The stock restore check chooses `BD MODERN` and captures its output. A separate assertion confirms that a stock picker without an offered row leaves a directly bound peer track alone.

**[D]** The emulator produced the expected binding and start counts, and captured PCM for both the pool sample and the restored stock voice. The trace showed the stock cancel flag for peer confirmation and the stock confirm flag when selecting `BD MODERN`. The list reopened on `SHOT` while bound and omitted it after switching the track to MIDI. The peer reported zero bus faults. The proof image had an 8.0 KB bootstrap margin and 113.1 KB of container room.

**[O]** The fixture supplies the label and the built-in peer renderer. Registering and retiring a loadable Shot renderer through the machine service, handling kit switches and unload, audible stock audition while a peer track is bound, and physical playback remain open.
