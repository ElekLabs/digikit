// Host oracle for the PLAITS 2FM, FORMANT and ADDITIVE ports (tests/plaits/test_fm_grain_additive_host.py): upstream
// Plaits' FMEngine, GrainEngine and AdditiveEngine, built from ../eurorack/plaits with tests/plaits/oracle/include
// earlier on the include path (plaits/dsp/dsp.h with kCorrectedSampleRate = 48000), as ws_oracle.cc does for
// ASYM-TRI. A C interface for ctypes: one engine per handle (engine 0 = FM, 1 = grain, 2 = additive), rendered in
// blocks of `size` (at most kMaxBlockSize) samples; and the upstream tables the port's lib/tables.c encodes.

#include <cstring>
#include <new>

#include "plaits/dsp/engine/additive_engine.h"
#include "plaits/dsp/engine/fm_engine.h"
#include "plaits/dsp/engine/grain_engine.h"
#include "plaits/resources.h"
#include "stmlib/dsp/units.h"

namespace {

struct Oracle {
  int which;
  plaits::FMEngine fm;
  plaits::GrainEngine grain;
  plaits::AdditiveEngine additive;
  char arena[1024];
  stmlib::BufferAllocator allocator;
};

plaits::Engine* engine(Oracle* o) {
  switch (o->which) {
    case 0: return &o->fm;
    case 1: return &o->grain;
    default: return &o->additive;
  }
}

}  // namespace

extern "C" {

int fga_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

// The memory starts zeroed (as Plaits' static engines do at power-up), then Init and Reset as Plaits' voice does
// when the engine is selected.
void fga_oracle_init(void* memory, int which) {
  std::memset(memory, 0, sizeof(Oracle));
  Oracle* o = new (memory) Oracle();
  o->which = which;
  o->allocator.Init(o->arena, sizeof(o->arena));
  engine(o)->Init(&o->allocator);
  engine(o)->Reset();
}

void fga_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, float* out, float* aux,
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

// Table k in lib/gen_lib_tables.py's order (sine, fm frequency quantizer, 4x downsampler FIR, pitch ratio high,
// pitch ratio low), entry i, as upstream's code reads it.
float fga_oracle_lut(int k, int i) {
  switch (k) {
    case 0: return plaits::lut_sine[i];
    case 1: return plaits::lut_fm_frequency_quantizer[i];
    case 2: return plaits::lut_4x_downsampler_fir[i];
    case 3: return stmlib::lut_pitch_ratio_high[i];
    default: return stmlib::lut_pitch_ratio_low[i];
  }
}

}  // extern "C"
