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

- [x] Play one known 48 kHz mono sample from the digikit store on the second ColdFire with the stock slots intact.

Map the peer's memory, boot transfer and audio dispatch on Syntakt 1.42. Choose a bounded digikit sample region and a way to publish a sample to the peer. Keep the existing stock payload and slot identities valid with all 64 slots populated. The peer section is currently read-only in Patchkit. Measure packed image size and bootstrap margin before choosing how to install a peer entry point. See [the peer CPU](../findings/136-syntakt-peer-cpu.md), [the stock boot transfer](../findings/142-syntakt-sample-storage.md#2-limits-and-what-the-peer-receives) and [the unpack bound](../findings/159-syntakt-inplace-unpack-bound.md).

Start with a boot preload and a single triggered voice. Check the captured signal against expected pitch and duration, verify stock playback with the adapter present, and exercise a missing or invalid sample. Record the memory budget and the measured frame cost. Stop at review if there is no safe region, dispatch point or image fit. Use the proof to define the smallest peer contract for loading a voice, passing parameters, publishing samples and retiring references.

Phase 2 verified a 40 ms pool sample through the Syntakt 1.42 emulator's stereo output. All 64 populated short slots and their data matched the baseline byte for byte, and the same stock digital note played on both images. Missing and invalid samples sent the stock peer image. The fixture is opt-in and loads from the card at boot. The full-size bank faults in the stock emulator, and physical playback and Twinshot audition remain open. Measurements and the proposed peer contract are in [the playback finding](../findings/160-syntakt-peer-sample-proof.md).

## Phase 3 — Build, transfer and load Shot

- [ ] Install Shot from Workbench on a compatible Syntakt image and trigger a mono file uploaded to the pool.

Start with the peer contract. Define how MAIN publishes a bounded PCM buffer, sends a trigger and parameters, and gets acknowledgment when the peer has retired a sample generation. Keep the file lease and card I/O on MAIN. The first checkpoint is one mono file published and triggered at runtime through this contract, with a safe replacement while a voice still references the previous sample.

- [x] Verify runtime publication, triggering and replacement in the Syntakt emulator. See [the runtime publication finding](../findings/161-syntakt-live-peer-audio.md).
- [x] Bind the peer voice to a digital track and verify that normal notes on that track start the sample. See [the note dispatch finding](../findings/162-syntakt-peer-note-dispatch.md).
- [x] Prove selection from the native Syntakt machine list with the peer audio fixture. See [the picker finding](../findings/163-syntakt-peer-machine-list.md).
- [ ] Register the machine with the machines service and admit and retire the peer renderer from a module package.
- [ ] Verify the Syntakt Shot package and Workbench installation.

Keep the `digikit/shot` module ID and `shot` machine ID. Share parameter and picker code where the machine contract permits. Keep the SHARC renderer for DT2/DN2 and add a Syntakt peer ColdFire renderer. Put firmware-facing code in the platform adapter. Reuse shared Rust file identity and lifecycle logic where its contract fits, with peer-specific memory and publication behind the adapter.

Shot's shared sources use the SDK. Keep the target renderer in `src/dsp/` and declare support in the manifest. Keep stock addresses, machine hooks, mailbox layout and mixer routing in the platform adapter. Test lease ownership, bounds and retirement on the host. Use emulator runs for the firmware and audio seams.

Extend Patchkit's per-device packing and runtime admission for the versioned peer payload. Add the Syntakt port to Shot once the payload can load. Derive Nx pack targets from the device registry and use the existing Workbench device library. Expose the required playback capability so an older Syntakt image gives a clear refusal. Verify that wrong-target and unsupported payloads are refused, then select Syntakt in Workbench, upload and verify Shot, enable it and trigger the uploaded sample. Run the existing DT2 and DN2 Shot checks for any shared code that changes. Scope additional Shot playback and page features against the peer contract and record their support in the module target status.

## Follow-up — Stereo Shot playback on Syntakt

- [ ] Play a stereo sample from the digikit pool with distinct left and right channels at the Syntakt's output.

Reuse the stereo PCM16 files Workbench already preserves in the pool. Determine how the peer can deliver separate left and right signals to MAIN's mixer, and measure the impact on available digital voices, peer memory and frame time. Extend the Shot renderer and the publication contract to carry both channels. Keep centered playback for mono files and preserve the stock mixer and slot behavior.

Verify with an asymmetric stereo fixture that makes channel swaps and fold-down audible and measurable. Check both outputs in the emulator and on hardware, and record the voice and frame budgets before enabling stereo for the Syntakt target.
