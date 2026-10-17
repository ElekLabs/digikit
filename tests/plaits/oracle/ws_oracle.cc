// Host oracle for the PLAITS ASYM-TRI port (tests/plaits/test_ws_host.py): upstream Plaits' WaveshapingEngine, built
// from ../eurorack/plaits with tests/plaits/oracle/include earlier on the include path (plaits/dsp/dsp.h with
// kCorrectedSampleRate = 48000), as va_oracle.cc does for VA. A C interface for ctypes: one engine per handle, rendered
// in blocks of `size` (at most kMaxBlockSize) samples; and the upstream tables the port's ws_tables.h encodes.

#include <new>

#include "plaits/dsp/engine/waveshaping_engine.h"
#include "plaits/resources.h"

namespace {

struct Oracle {
  plaits::WaveshapingEngine engine;
  char arena[1024];
  stmlib::BufferAllocator allocator;
};

}  // namespace

extern "C" {

int ws_oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

void ws_oracle_init(void* memory) {
  Oracle* o = new (memory) Oracle();
  o->allocator.Init(o->arena, sizeof(o->arena));
  o->engine.Init(&o->allocator);
  o->engine.Reset();
}

void ws_oracle_render(void* memory, float note, float harmonics, float timbre, float morph, float* out, float* aux,
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
  o->engine.Render(p, out, aux, static_cast<size_t>(size), &already_enveloped);
}

// Table k (the order of gen_ws_tables.py: sine, fold, fold_2, then the five waveshapers), entry i, as upstream's
// code reads it (an int16 entry v as static_cast<float>(v) / 32768.0f).
float ws_oracle_lut(int k, int i) {
  switch (k) {
    case 0: return plaits::lut_sine[i];
    case 1: return plaits::lut_fold[i];
    case 2: return plaits::lut_fold_2[i];
    default: return static_cast<float>(plaits::lookup_table_i16_table[k - 3][i]) / 32768.0f;
  }
}

}  // extern "C"
