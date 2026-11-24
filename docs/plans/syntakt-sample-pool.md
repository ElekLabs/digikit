# Syntakt sample pool and Shot

Syntakt OS 1.42 keeps 64 numbered stock sample slots. Its patched firmware also has a 512 MiB digikit store shared by modules and their files. Workbench can expose both sample locations using the existing transports. Playback from the digikit store needs a separate path to the second ColdFire. Shot is the first intended client. See [sample storage](../findings/142-syntakt-sample-storage.md), [the digikit store on hardware](../findings/154-syntakt-card-capacity-store.md#6-the-store-on-the-unit-d) and [Shot](../../modules/machine/digikit/shot/README.md).

Work through the phases in order. Record the checks and mark the phase complete in this plan when its acceptance is met. Present the diff and results with the phase uncommitted. Wait for Em's approval, then commit the phase before starting the next one.

## Phase 1 — Transfer samples and verify persistence

- [x] Expose **Sample Slots** and **digikit Sample Pool** in Workbench and prove storage on Syntakt 1.42.

Declare the sample locations in device data. Resolve them against the connected session and reuse `SamplesBackend` for the stock Data API and the digikit `samples/` folder. A patched Syntakt shows both folders, and a stock Syntakt opens its single stock location directly. DT2 1.17 and DN2 1.12 continue to open their existing single locations. The same pane works with Emulator and USB. The pool folder describes file storage until playback is available. The stock slot view keeps its restart behavior.

Give each operation an explicit destination consisting of the target binding, sample location and relative folder. Virtual roots are navigation. Uploads and mutations require an open, writable location. Refreshes and late replies stay with the location that initiated them. Use the existing native sample converter, backend hash verification and shared store capacity. This phase uses the firmware and guard already in the Syntakt patch set.

Check host routing and stale reads, including an absent runtime and an unavailable or read-only store. Extend `core/runtime-tests:test-device--samples-store-live` to Syntakt. In the emulator, upload a generated WAV, verify the stored bytes, restart and verify again. Compare the stock slot listing across the pool operation. Repeat the transfer on a connected, already-patched Syntakt when available, using a dedicated test folder. Record hardware status at the review point.

Phase 1 verification passed on the Syntakt 1.42 and Digitone II 1.12 emulator targets. The Syntakt run verified the stored hash before and after restart, matched the mirrored bytes against the upload, and compared all 64 stock slot entries across the operation. Host checks covered the two destinations, reloads, permission changes on the same MIDI session, and late replies. They also verified that a missed runtime INFO reply blocks pool writes while the stock slots remain usable. Physical USB verification is pending. The MIDI port scan found no connected Syntakt.

## Phase 2 — Prove sample playback on the peer

- [ ] Play one known 48 kHz mono sample from the digikit store on the second ColdFire with the stock slots intact.

Map the peer's memory, boot transfer and audio dispatch on Syntakt 1.42. Choose a bounded digikit sample region and a way to publish a sample to the peer. Keep the existing stock payload and slot identities valid with all 64 slots populated. The peer section is currently read-only in Patchkit. Measure packed image size and bootstrap margin before choosing how to install a peer entry point. See [the peer CPU](../findings/136-syntakt-peer-cpu.md), [the stock boot transfer](../findings/142-syntakt-sample-storage.md#2-limits-and-what-the-peer-receives) and [the unpack bound](../findings/159-syntakt-inplace-unpack-bound.md).

Start with a boot preload and a single triggered voice. Check the captured signal against expected pitch and duration, verify stock playback with the adapter present, and exercise a missing or invalid sample. Record the memory budget and the measured frame cost. Stop at review if there is no safe region, dispatch point or image fit. Use the proof to define the smallest peer contract for loading a voice, passing parameters, publishing samples and retiring references.

## Phase 3 — Build, transfer and load Shot

- [ ] Install Shot from Workbench on a compatible Syntakt image and trigger a file uploaded to the pool.

Keep the `digikit/shot` module ID and `shot` machine ID. Share parameter and picker code where the machine contract permits. Keep the SHARC renderer for DT2/DN2 and add a Syntakt peer ColdFire renderer. Put firmware-facing code in the platform adapter. Reuse shared Rust file identity and lifecycle logic where its contract fits, with peer-specific memory and publication behind the adapter.

Extend Patchkit's per-device packing and runtime admission for the versioned peer payload. Add the Syntakt port to Shot once the payload can load. Derive Nx pack targets from the device registry and use the existing Workbench device library. Expose the required playback capability so an older Syntakt image gives a clear refusal. Verify that wrong-target and unsupported payloads are refused, then select Syntakt in Workbench, upload and verify Shot, enable it and trigger the uploaded sample. Run the existing DT2 and DN2 Shot checks for any shared code that changes. Scope additional Shot playback and page features against the peer contract and record their support in the module target status.
