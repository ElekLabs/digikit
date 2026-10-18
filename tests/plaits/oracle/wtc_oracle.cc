// Host oracle for the PLAITS WAVETABLE and CHORD ports (tests/plaits/test_wavetable_chord_host.py): upstream Plaits'
// WavetableEngine and ChordEngine, built from ../eurorack/plaits with tests/plaits/oracle/include earlier on the
// include path (plaits/dsp/dsp.h with kCorrectedSampleRate = 48000), as fga_oracle.cc does for models 2..4. A C
// interface for ctypes: one engine per handle (engine 0 = wavetable, 1 = chord), rendered in blocks of `size` (at most
// kMaxBlockSize) samples; and the upstream tables and state the port's generated tables encode.
//
// The engines' private members (WavetableEngine::wave_map_, ChordBank::ratios_ and note_count_) are read by
// declaring `private` public for the upstream headers (test-only code; the standard headers are included first).

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>

#define private public
#include "plaits/dsp/chords/chord_bank.h"
#include "plaits/dsp/engine/chord_engine.h"
#include "plaits/dsp/engine/wavetable_engine.h"
#undef private
#include "plaits/resources.h"
#include "stmlib/dsp/units.h"

namespace {

struct Oracle {
  int which;
  plaits::WavetableEngine wavetable;
  plaits::ChordEngine chord;
  // WavetableEngine::Init allocates 64 wave pointers and LoadUserData writes 256 (in Plaits the arena is shared and
  // much larger): the arena holds them.
  alignas(16) char arena[8192];
  stmlib::BufferAllocator allocator;
};

plaits::Engine* engine(Oracle* o) {
  return o->which == 0 ? static_cast<plaits::Engine*>(&o->wavetable) : &o->chord;
}

}  // namespace

extern "C" {

int wtc_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

// The memory starts zeroed (as Plaits' static engines do at power-up), then Init, LoadUserData(NULL) and Reset as
// Plaits' voice does when the engine is selected (UserData::ptr is NULL without a user wavetable).
void wtc_oracle_init(void* memory, int which) {
  std::memset(memory, 0, sizeof(Oracle));
  Oracle* o = new (memory) Oracle();
  o->which = which;
  o->allocator.Init(o->arena, sizeof(o->arena));
  engine(o)->Init(&o->allocator);
  engine(o)->LoadUserData(NULL);
  engine(o)->Reset();
}

void wtc_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, float* out, float* aux,
                       int size) {
  Oracle* o = static_cast<Oracle*>(memory);
  plaits::EngineParameters p;
  p.trigger = plaits::TRIGGER_LOW;
  p.note = note;
  p.timbre = timbre;
  p.morph = morph;
  p.harmonics = harmonics;
  p.accent = 0.0f;
  bool already_enveloped = false;
  engine(o)->Render(p, out, aux, static_cast<size_t>(size), &already_enveloped);
}

// wav_integrated_waves[i] as upstream's code reads it (static_cast<float>).
float wtc_oracle_wave(int i) { return static_cast<float>(plaits::wav_integrated_waves[i]); }
int wtc_oracle_wave_count(void) { return WAV_INTEGRATED_WAVES_SIZE; }

// wave_map_[i] of a wavetable handle after LoadUserData(NULL), as an offset (entries) into wav_integrated_waves.
int wtc_oracle_wave_map(void* memory, int i) {
  Oracle* o = static_cast<Oracle*>(memory);
  return static_cast<int>(o->wavetable.wave_map_[i] - plaits::wav_integrated_waves);
}

// ChordBank::ratios_[i] (i < 44) and note_count_[i] (i < 11) of a chord handle after Reset.
float wtc_oracle_chord_ratio(void* memory, int i) {
  return static_cast<Oracle*>(memory)->chord.chords_.ratios_[i];
}
int wtc_oracle_chord_note_count(void* memory, int i) {
  return static_cast<Oracle*>(memory)->chord.chords_.note_count_[i];
}

// A ChordBank of its own: set_chord(parameter), then ComputeChordInversion(inversion); writes ratios[5],
// amplitudes[5], returns the mask; *index receives chord_index().
int wtc_oracle_chord_inversion(float parameter, float inversion, float* ratios, float* amplitudes, int* index) {
  alignas(16) static char arena[1024];
  stmlib::BufferAllocator allocator;
  allocator.Init(arena, sizeof(arena));
  plaits::ChordBank bank;
  bank.Init(&allocator);
  bank.Reset();
  bank.set_chord(parameter);
  *index = bank.chord_index();
  return bank.ComputeChordInversion(inversion, ratios, amplitudes);
}

}  // extern "C"
