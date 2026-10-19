// Host oracle for continuous rendering across note-ons (tests/plaits/test_continuous_host.py): upstream Plaits'
// engines of PLAITS models 0..7 (VirtualAnalogEngine, WaveshapingEngine, FMEngine, GrainEngine, AdditiveEngine,
// WavetableEngine, ChordEngine, SpeechEngine), 12 (ModalEngine) and 21 (WaveTerrainEngine), built from ../eurorack/plaits with tests/plaits/oracle/include earlier on the include
// path (plaits/dsp/dsp.h with kCorrectedSampleRate = 48000). A C interface for ctypes.
//
// Plaits' Voice (plaits/dsp/voice.cc) calls each engine's Init once at power-up; on an engine change it calls
// LoadUserData(NULL) (UserData::ptr is NULL without user data; the FM-patch case is the six-op engines only) and Reset;
// it never re-initialises an engine at a trigger, which reaches the engine only as EngineParameters.trigger.
// run_oracle_select(which) is what the PLAITS wrapper does at a model change on its shared union: the new engine from
// zeroed memory, Init, LoadUserData(NULL), Reset (and stmlib's Random seeded with its power-up state: SPEECH's LPC
// synth draws noise from it, and the port keeps one state per voice from 0x21). run_oracle_render passes `trigger`
// through to the engine (models 0..6 do not read it; SPEECH does), with the given accent (the wrapper's, from the
// note-on's velocity; read by SPEECH) and SPEECH's prosody amount and speed set to 0 before each render, as Plaits'
// Voice sets them from its attenuverters at noon (plaits/dsp/voice.cc).
//
// Plaits' output stage (run_oracle_post*): the two stmlib::Limiter of Voice's out_post_processor_ and
// aux_post_processor_ (ChannelPostProcessor, plaits/dsp/voice.h), kept apart from the engines because they outlive an
// engine change: Init at power-up, and at an engine change Reset of the OUT one only (voice.cc). Process is
// ChannelPostProcessor::Process with the low-pass gate bypassed, in float full-scale units: a negative gain runs the
// limiter in place at -gain, a positive one multiplies, then the DAC step's Clip16 as a clamp to [-1, 1] (its
// 16-bit quantisation and one-LSB offset dropped, as in the port).

#include <cstring>
#include <new>

#include "plaits/dsp/engine/additive_engine.h"
#include "plaits/dsp/engine/chord_engine.h"
#include "plaits/dsp/engine/fm_engine.h"
#include "plaits/dsp/engine/grain_engine.h"
#include "plaits/dsp/engine/modal_engine.h"
#include "plaits/dsp/engine/speech_engine.h"
#include "plaits/dsp/engine/virtual_analog_engine.h"
#include "plaits/dsp/engine/waveshaping_engine.h"
#include "plaits/dsp/engine/wavetable_engine.h"
#include "plaits/dsp/engine2/wave_terrain_engine.h"
#include "stmlib/dsp/limiter.h"
#include "stmlib/utils/random.h"

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
  plaits::SpeechEngine speech;
  plaits::ModalEngine modal;
  plaits::WaveTerrainEngine terrain;
  // SpeechEngine::Init allocates 1024 LPC frames of 14 bytes, 32 word boundaries and two temporary buffers.
  alignas(16) char arena[32768];
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
    case 6: return &o->chord;
    case 12: return &o->modal;
    case 21: return &o->terrain;
    default: return &o->speech;
  }
}

}  // namespace

extern "C" {

int run_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

// Model `which` (0..7, 12, 21) from zeroed memory: Init, LoadUserData(NULL), Reset.
void run_oracle_select(void* memory, int which) {
  std::memset(memory, 0, sizeof(Oracle));
  Oracle* o = new (memory) Oracle();
  o->which = which;
  o->allocator.Init(o->arena, sizeof(o->arena));
  engine(o)->Init(&o->allocator);
  engine(o)->LoadUserData(NULL);
  engine(o)->Reset();
  stmlib::Random::Seed(0x21);
}

// Plaits' trigger bits (plaits/dsp/engine/engine.h TriggerState): 0 LOW, 1 RISING_EDGE, 2 UNPATCHED, 4 HIGH.
void run_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, int trigger,
                       float accent, float* out, float* aux, int size) {
  Oracle* o = static_cast<Oracle*>(memory);
  plaits::EngineParameters p;
  p.trigger = trigger;
  p.note = note;
  p.timbre = timbre;
  p.morph = morph;
  p.harmonics = harmonics;
  p.accent = accent;
  o->speech.set_prosody_amount(0.0f);
  o->speech.set_speed(0.0f);
  bool already_enveloped = false;
  engine(o)->Render(p, out, aux, static_cast<size_t>(size), &already_enveloped);
}

struct Post {
  stmlib::Limiter out;
  stmlib::Limiter aux;
};

int run_oracle_post_size(void) { return static_cast<int>(sizeof(Post)); }

// Voice::Init: out_post_processor_.Init(), aux_post_processor_.Init() (each Reset()s its limiter).
void run_oracle_post_init(void* memory) {
  Post* q = new (memory) Post();
  q->out.Init();
  q->aux.Init();
}

// An engine change: out_post_processor_.Reset() (limiter_.Init()).
void run_oracle_post_reset(void* memory) { static_cast<Post*>(memory)->out.Init(); }

static void process(stmlib::Limiter* limiter, float gain, float* in, int size) {
  if (gain < 0.0f) {
    limiter->Process(-gain, in, static_cast<size_t>(size));
  }
  const float post_gain = gain < 0.0f ? 1.0f : gain;
  for (int i = 0; i < size; ++i) {
    float y = in[i] * post_gain;
    CONSTRAIN(y, -1.0f, 1.0f);  // Clip16 in full-scale units
    in[i] = y;
  }
}

void run_oracle_post(void* memory, float out_gain, float aux_gain, float* out, float* aux, int size) {
  Post* q = static_cast<Post*>(memory);
  process(&q->out, out_gain, out, size);
  process(&q->aux, aux_gain, aux, size);
}

}  // extern "C"
