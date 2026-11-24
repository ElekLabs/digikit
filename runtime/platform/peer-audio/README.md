# core/peer-audio

An opt-in Syntakt 1.42 adapter. The [finding](../../../docs/findings/161-syntakt-live-peer-audio.md) describes the versioned mailbox, DDR reservation and remaining machine integration. The firmware entry, mailbox and stock program operands live here. A content module uses the eventual SDK machine and sample contracts.

Run the host checks from the repository root.

```sh
pnpm nx:pilot run core/peer-audio:test-host
pnpm nx:pilot run core/peer-audio:rust-tests
```

The emulator proof uses two native mono files in the store, `samples/long` (31,200 frames at 440 Hz) and `samples/next` (960 frames at 660 Hz). Upload their WAVs with Workbench, then use that emulator card image. Build Patchkit and the proof image.

```sh
pnpm nx:pilot run patchkit:cli
dist/patchkit/patchkit build core/peer-audio test/peer-audio
dist/patchkit/patchkit apply --firmware firmware/Syntakt_OS1.42.syx \
  --modules core/runtime,core/card-guard,core/ui,core/health,core/input,core/peer-image,test/peer-audio \
  --image-only --plan out/peer-audio.plan.json -o out/st-live-peer-audio.syx
proof=$(jq -r '.symbols["test/peer-audio@0.1.0:peer_audio_proof"]' out/peer-audio.plan.json)
result=$(jq -r '.symbols["test/peer-audio@0.1.0:peer_audio_proof_result"]' out/peer-audio.plan.json)
DIGI_CF_IPS=190000000 node tools/nx/rust-env.mjs -- \
  cargo run --release -p elektron-native-boot --example panel_drive -- \
  out/st-live-peer-audio.syx --after 111M --max 3400M --card-image out/st-card.img \
  --call-at 0x4000b2d8 --call-fn "$proof" \
  --steps "peer,send:5073616d706c65732f6c6f6e6700,wait:1800M,send:53,peek:$result:4,midi:903c64,wait:18M,pcmreset,send:54,wait:5M,send:5073616d706c65732f6e65787400,wait:65M,send:53,peek:$result:4,peek:0x10000600:52,send:5073616d706c65732f6c6f6e6700,peek:$result:4,wait:95M,peek:0x10000600:52,send:54,peek:$result:4,wait:12M,wav:out/st-live-peer-audio.wav,peer"
```

The status calls return generations 1 and 2. During the busy check the mailbox lists published bank 2 and playing bank 1, and the proof result is `fffffff4` (busy). The next snapshot shows playing bank zero, and the replacement trigger returns zero.
