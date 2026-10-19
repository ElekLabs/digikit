// Host oracle for the PLAITS SPEECH port (tests/plaits/test_speech_host.py): upstream Plaits' SpeechEngine, built from
// ../eurorack/plaits with tests/plaits/oracle/include earlier on the include path (plaits/dsp/dsp.h with
// kCorrectedSampleRate = 48000), as the other oracles. A C interface for ctypes: one engine per handle, rendered in
// blocks of `size` (at most kMaxBlockSize) samples, with Plaits' trigger bits and accent and the prosody amount and
// speed Plaits' Voice sets before each render (plaits/dsp/voice.cc); and the upstream tables the port's generated
// tables encode (the engine's private members are read by declaring `private` public for the upstream headers;
// test-only code, the standard headers are included first).
//
// stmlib's Random has one global state (Random::rng_state_, 0x21 at power-up); the LPC synth draws its noise from it.
// speech_oracle_init seeds it with 0x21, as the port's per-voice state starts: one handle at a time.

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>

#define private public
#include "plaits/dsp/engine/speech_engine.h"
#include "plaits/dsp/speech/lpc_speech_synth_controller.h"
#include "plaits/dsp/speech/lpc_speech_synth_words.h"
#include "plaits/dsp/speech/naive_speech_synth.h"
#include "plaits/dsp/speech/sam_speech_synth.h"
#undef private
#include "plaits/resources.h"
#include "stmlib/utils/random.h"

namespace {

struct Oracle {
  plaits::SpeechEngine speech;
  // SpeechEngine::Init allocates 1024 LPC frames (14 bytes each), 32 word boundaries and two temporary buffers.
  alignas(16) char arena[32768];
  stmlib::BufferAllocator allocator;
};

const plaits::LPCSpeechSynth::Frame& frame_of(const plaits::LPCSpeechSynth::Frame* frames, int i) { return frames[i]; }

int field(const plaits::LPCSpeechSynth::Frame& f, int k) {
  switch (k) {
    case 0: return f.energy;
    case 1: return f.period;
    case 2: return f.k0;
    case 3: return f.k1;
    case 4: return f.k2;
    case 5: return f.k3;
    case 6: return f.k4;
    case 7: return f.k5;
    case 8: return f.k6;
    case 9: return f.k7;
    case 10: return f.k8;
    default: return f.k9;
  }
}

}  // namespace

extern "C" {

int speech_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

// Zeroed memory (Plaits' static engines at power-up), Init, LoadUserData(NULL) and Reset (Plaits' voice at the engine's
// selection); Random seeded with its power-up state.
void speech_oracle_init(void* memory) {
  std::memset(memory, 0, sizeof(Oracle));
  Oracle* o = new (memory) Oracle();
  o->allocator.Init(o->arena, sizeof(o->arena));
  o->speech.Init(&o->allocator);
  o->speech.LoadUserData(NULL);
  o->speech.Reset();
  stmlib::Random::Seed(0x21);
}

// Plaits' trigger bits (plaits/dsp/engine/engine.h TriggerState): 0 LOW, 1 RISING_EDGE, 2 UNPATCHED, 4 HIGH. Returns
// already_enveloped.
int speech_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, int trigger,
                         float accent, float prosody_amount, float speed, float* out, float* aux, int size) {
  Oracle* o = static_cast<Oracle*>(memory);
  o->speech.set_prosody_amount(prosody_amount);
  o->speech.set_speed(speed);
  plaits::EngineParameters p;
  p.trigger = trigger;
  p.note = note;
  p.timbre = timbre;
  p.morph = morph;
  p.harmonics = harmonics;
  p.accent = accent;
  bool already_enveloped = false;
  o->speech.Render(p, out, aux, static_cast<size_t>(size), &already_enveloped);
  return already_enveloped ? 1 : 0;
}

// The LPC controller's playback state (for the tests' choice of scenarios).
int speech_oracle_playback_frame(void* memory) {
  return static_cast<Oracle*>(memory)->speech.lpc_speech_synth_controller_.playback_frame_;
}

// NaiveSpeechSynth::phonemes_[p][r].formant[i]: k 0 frequency, 1 amplitude.
int speech_oracle_naive_phoneme(int p, int r, int i, int k) {
  const plaits::NaiveSpeechSynth::Formant& f = plaits::NaiveSpeechSynth::phonemes_[p][r].formant[i];
  return k == 0 ? f.frequency : f.amplitude;
}

// SAMSpeechSynth::phonemes_[p].formant[i] (p < 18): k 0 frequency, 1 amplitude index; and formant_amplitude_lut.
int speech_oracle_sam_phoneme(int p, int i, int k) {
  const plaits::SAMSpeechSynth::Formant& f = plaits::SAMSpeechSynth::phonemes_[p].formant[i];
  return k == 0 ? f.frequency : f.amplitude;
}
float speech_oracle_sam_amplitude(int i) { return plaits::SAMSpeechSynth::formant_amplitude_lut[i]; }

// LPCSpeechSynthController::phonemes_[p], field k (0 energy, 1 period, 2..11 k0..k9).
int speech_oracle_lpc_phoneme(int p, int k) {
  return field(frame_of(plaits::LPCSpeechSynthController::phonemes_, p), k);
}

// A word bank of its own, Load(bank): the number of frames and words, frame i's field k and boundary i.
namespace {
alignas(16) char bank_arena[32768];
stmlib::BufferAllocator bank_allocator;
plaits::LPCSpeechSynthWordBank bank;
}  // namespace

void speech_oracle_load_bank(int b) {
  bank_allocator.Init(bank_arena, sizeof(bank_arena));
  bank.Init(plaits::word_banks_, LPC_SPEECH_SYNTH_NUM_WORD_BANKS, &bank_allocator);
  bank.Load(b);
}
int speech_oracle_bank_num_frames(void) { return bank.num_frames(); }
int speech_oracle_bank_num_words(void) { return bank.num_words_; }
int speech_oracle_bank_frame(int i, int k) { return field(frame_of(bank.frames(), i), k); }
int speech_oracle_bank_boundary(int i) { return bank.word_boundaries_[i]; }

// lut_lpc_excitation_pulse[i].
int speech_oracle_excitation_pulse(int i) { return plaits::lut_lpc_excitation_pulse[i]; }

}  // extern "C"
