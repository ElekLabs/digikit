// Host oracle for continuous rendering across note-ons (tests/plaits/test_continuous_host.py): upstream Plaits'
// engines of PLAITS models 0..6 (VirtualAnalogEngine, WaveshapingEngine, FMEngine, GrainEngine, AdditiveEngine,
// WavetableEngine, ChordEngine), built from ../eurorack/plaits with tests/plaits/oracle/include earlier on the include
// path (plaits/dsp/dsp.h with kCorrectedSampleRate = 48000). A C interface for ctypes.
//
// Plaits' Voice (plaits/dsp/voice.cc) calls each engine's Init once at power-up; on an engine change it calls
// LoadUserData(NULL) (UserData::ptr is NULL without user data; the FM-patch case is the six-op engines only) and Reset;
// it never re-initialises an engine at a trigger, which reaches the engine only as EngineParameters.trigger.
// run_oracle_select(which) is what the PLAITS wrapper does at a model change on its shared union: the new engine from
// zeroed memory, Init, LoadUserData(NULL), Reset. run_oracle_render passes `trigger` through to the engine (models
// 0..6 do not read it).

#include <cstring>
#include <new>

#include "plaits/dsp/engine/additive_engine.h"
#include "plaits/dsp/engine/chord_engine.h"
#include "plaits/dsp/engine/fm_engine.h"
#include "plaits/dsp/engine/grain_engine.h"
#include "plaits/dsp/engine/virtual_analog_engine.h"
#include "plaits/dsp/engine/waveshaping_engine.h"
#include "plaits/dsp/engine/wavetable_engine.h"

namespace {

struct Oracle {
  int which;
  plaits::VirtualAnalogEngine va;
  plaits::WaveshapingEngine ws;
  plaits::FMEngine fm;
  plaits::GrainEngine grain;
  plaits::AdditiveEngine additive;
  plaits::WavetableEngine wavetable;
  plaits::ChordEngine chord;
  alignas(16) char arena[8192];
  stmlib::BufferAllocator allocator;
};

plaits::Engine* engine(Oracle* o) {
  switch (o->which) {
    case 0: return &o->va;
    case 1: return &o->ws;
    case 2: return &o->fm;
    case 3: return &o->grain;
    case 4: return &o->additive;
    case 5: return &o->wavetable;
    default: return &o->chord;
  }
}

}  // namespace

extern "C" {

int run_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

// Model `which` (0..6) from zeroed memory: Init, LoadUserData(NULL), Reset.
void run_oracle_select(void* memory, int which) {
  std::memset(memory, 0, sizeof(Oracle));
  Oracle* o = new (memory) Oracle();
  o->which = which;
  o->allocator.Init(o->arena, sizeof(o->arena));
  engine(o)->Init(&o->allocator);
  engine(o)->LoadUserData(NULL);
  engine(o)->Reset();
}

// Plaits' trigger bits (plaits/dsp/engine/engine.h TriggerState): 0 LOW, 1 RISING_EDGE, 2 UNPATCHED, 4 HIGH.
void run_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, int trigger, float* out,
                       float* aux, int size) {
  Oracle* o = static_cast<Oracle*>(memory);
  plaits::EngineParameters p;
  p.trigger = trigger;
  p.note = note;
  p.timbre = timbre;
  p.morph = morph;
  p.harmonics = harmonics;
  p.accent = 0.0f;
  bool already_enveloped = false;
  engine(o)->Render(p, out, aux, static_cast<size_t>(size), &already_enveloped);
}

}  // extern "C"
