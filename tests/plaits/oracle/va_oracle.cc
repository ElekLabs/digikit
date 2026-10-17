// Host oracle for the PLAITS VA port (tests/plaits/test_va_host.py): upstream Plaits' VirtualAnalogEngine, built
// from ../eurorack/plaits with tests/plaits/oracle/include earlier on the include path, so its plaits/dsp/dsp.h (the
// only difference: kCorrectedSampleRate = 48000) replaces upstream's. A C interface for ctypes: one engine instance
// per handle, rendered in blocks of `size` (at most kMaxBlockSize) samples.

#include <new>

#include "plaits/dsp/engine/virtual_analog_engine.h"

namespace {

struct Oracle {
  plaits::VirtualAnalogEngine engine;
  char arena[1024];
  stmlib::BufferAllocator allocator;
};

}  // namespace

extern "C" {

int oracle_size(void) { return static_cast<int>(sizeof(Oracle)); }

float oracle_a0(void) { return plaits::a0; }

void oracle_init(void* memory) {
  Oracle* o = new (memory) Oracle();
  o->allocator.Init(o->arena, sizeof(o->arena));
  o->engine.Init(&o->allocator);
  o->engine.Reset();
}

void oracle_render(void* memory, float note, float harmonics, float timbre, float morph, float* out, float* aux,
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

}  // extern "C"
