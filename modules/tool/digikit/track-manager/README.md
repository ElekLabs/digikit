# digikit/track-manager

Gives the tracks names. A run-time module (`.dkm`); it needs `core/ui` and `core/services/shortcuts`.

- SETTINGS > TRACK MANAGER opens a grid of the tracks (rows of 8, from the device's track count). The screen opens on the active track.
  Keys: [LEFT] [RIGHT] one track, wrapping; [UP] [DOWN] the other row (a missing cell goes to the last track of that row); [TRIG n] jumps to track n;
  [YES] opens the stock naming screen (up to 15 characters; an empty name removes the name); [NO] closes. A bar under a number marks a named track.
- Hold [TRK] and press [YES] on the stock screen: a strip shows `NN  NAME` of the active track (`TRACK NN` when it has no name) until a key is released.
  The stock track selection keeps working while it is held.

Where the names live: the module's setting `names` (version byte 1, track count, then per track a length byte and the characters). A value with
another version or track count loads as no names. The names are one list for the device for now; they become per project when the project service
lands.

The active track comes from one function, `active_track` (src/c/active_track.c), that uses the debug import for now.

Tests: `pnpm nx:pilot run digikit/track-manager:test-host` builds the C test driver of `track_names` and `grid_cursor` with `cc` and runs it.
