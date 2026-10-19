/* PLAITS for the Digitakt II: the voice wrapper around the Plaits engines (plain C99, float only).
 *
 * Built twice from the same source: on the host by tests/plaits (ctypes) and by Selache's C compiler into the DSP
 * object of machines/plaits-48k and machines/plaits-96k, where src/dsp/plaits_entry.s calls plaits_render from the
 * core/dsp-inline voice hook. Pointers are byte addresses on the SHARC+ (Selache's C), as are the Digitakt's buffers.
 */
#ifndef PLAITS_PLAITS_H
#define PLAITS_PLAITS_H

#include "engines/va.h"
#include "engines/waveshaping.h"
#include "engines/fm.h"
#include "engines/grain.h"
#include "engines/additive.h"
#include "engines/wavetable.h"
#include "engines/chord.h"
#include "engines/speech.h"
#include "engines/modal.h"
#include "engines/terrain.h"
#include "halfband.h"

/* Plaits sub-block: two per Digitakt block of 32 samples at 48 kHz (Plaits' own control rate is 4 kHz, here 3 kHz). */
#define PLAITS_SUB 16
#define PLAITS_BLOCK 32

/* Model numbers are permanent and follow Plaits' own order (docs/design/2026-10-17-plaits-machine-design.md, the
 * owner's instruction of 2026-10-17): 0 VA, 1 ASYM-TRI (waveshaping), 2 2FM, 3 FORMANT (grain), 4 ADDITIVE,
 * 5 WAVETABLE, 6 CHORD, 7 SPEECH, 8 SWARM, 9 NOISE, 10 DUST (particle), 11 STRING, 12 RINGS (modal), 13 KICK,
 * 14 SNARE, 15 HIHAT, 16 VCF-VA, 17 PHASE-DIST, 18 DX7 1, 19 DX7 2, 20 DX7 3, 21 TERRAIN, 22 STRING MACHINE,
 * 23 SQUARE (chiptune). Ported: 0 to 7, 12 and 21; the others render silence. */
#define PLAITS_MODEL_VA 0
#define PLAITS_MODEL_ASYM_TRI 1
#define PLAITS_MODEL_2FM 2
#define PLAITS_MODEL_FORMANT 3
#define PLAITS_MODEL_ADDITIVE 4
#define PLAITS_MODEL_WAVETABLE 5
#define PLAITS_MODEL_CHORD 6
#define PLAITS_MODEL_SPEECH 7
#define PLAITS_MODEL_RINGS 12
#define PLAITS_MODEL_TERRAIN 21

/* One block's controls, as the entry shim reads them from the DSP's frame copy (16-bit row values converted to
 * float; the conversion is exact) and from the voice record. */
typedef struct {
    float pitch;     /* frame halfword 1 + t: note << 8 */
    float tune;      /* machine slot 0 (TUNE row 0xf8): 0x4000 = no transpose, 0x100 per semitone */
    float model;     /* slot 1: 0 .. 0x1700, the model number << 8 */
    float harmonics; /* slot 2: 0 .. 0x7f00 */
    float timbre;    /* slot 4 */
    float morph;     /* slot 5 */
    float mix;       /* slot 6: 0 = OUT, 0x7f00 = AUX */
    float velocity;  /* frame halfword 0x1a + t: velocity << 8 (0x6400 = 100; finding 06) */
    int active;      /* the voice record's ACTIVE byte (+0x1b8) */
} PlaitsFrame;

/* The one-pole DC blocker at the voice output (about 5 Hz). */
typedef struct {
    float x1, y1;
} PlaitsDcBlock;

/* The engines of one voice share one state (Plaits shares one arena): the union is the size of the largest engine.
 * The engine of the model latched at the last note-on owns it; a note-on that changes the model initialises it for
 * the new model.
 * Access it only through PL_VA / PL_WS / PL_FM / PL_GRAIN / PL_ADD / PL_WT / PL_CHORD / PL_SPEECH / PL_MODAL / PL_TERRAIN, a pointer to the union converted to the member's type (C99 6.7.2.1: it points
 * to each member), never as `engine.ws`: Selache's C compiler places each union member after the previous one, as in
 * a struct (docs/findings/20-patch-toolchains.md), so `&v->engine.ws` would be v + sizeof(VA), inside out[]. Its
 * sizeof of the union is right (the largest member). */
typedef union {
    VA va;
    WS ws;
    FM fm;
    Grain grain;
    Additive additive;
    Wavetable wavetable;
    Chord chord;
    Speech speech;
    Modal modal;
    Terrain terrain;
} PlaitsEngine;
#define PL_VA(v) ((VA *)(void *)&(v)->engine)
#define PL_WS(v) ((WS *)(void *)&(v)->engine)
#define PL_FM(v) ((FM *)(void *)&(v)->engine)
#define PL_GRAIN(v) ((Grain *)(void *)&(v)->engine)
#define PL_ADD(v) ((Additive *)(void *)&(v)->engine)
#define PL_WT(v) ((Wavetable *)(void *)&(v)->engine)
#define PL_CHORD(v) ((Chord *)(void *)&(v)->engine)
#define PL_SPEECH(v) ((Speech *)(void *)&(v)->engine)
#define PL_MODAL(v) ((Modal *)(void *)&(v)->engine)
#define PL_TERRAIN(v) ((Terrain *)(void *)&(v)->engine)

/* Per-voice state. */
typedef struct {
    PlaitsEngine engine;
    float out[PLAITS_SUB]; /* the engine's OUT and AUX for one sub-block */
    float aux[PLAITS_SUB];
    PlaitsDcBlock dc;
    float out_peak;        /* Plaits' output limiters (stmlib Limiter peak_): OUT, reset with the engine; */
    float aux_peak;        /* AUX, set once, at the voice's first note-on (plaits.c post) */
    float accent;          /* Plaits' accent, from the velocity latched at the last note-on */
    int model;             /* latched at note-on */
    int armed;             /* an engine was initialised since the state was cleared (zero at load) */
    HalfBand hb;           /* the 2x interpolator, used when the output rate is 96 kHz */
    float buf48[PLAITS_BLOCK];
} PlaitsVoice;

/* Render one Digitakt block into `out`: n = 32 samples at rate 48000 (written as they are), or n = 64 at 96000 (32
 * rendered at 48 kHz, then hb_up2). `trig` is the voice's trigger-latch byte (non-zero in the block of a note-on):
 * it latches the model, and initialises the engine only at the voice's first note-on or when the model changed
 * (otherwise the engine runs on, as in Plaits; plaits.c note_on). A voice renders only when it was triggered since its state was
 * last cleared and its ACTIVE byte is set, and only when its latched model has an engine (VA 0, ASYM-TRI 1, 2FM 2,
 * FORMANT 3, ADDITIVE 4, WAVETABLE 5, CHORD 6, SPEECH 7, RINGS 12, TERRAIN 21);
 * otherwise the block is zeros.
 * Returns 1 when it rendered the engine, else 0. */
int plaits_render(PlaitsVoice *v, const PlaitsFrame *f, int trig, float *out, int n, int rate);

/* plaits_render for the voice state of track 0..15 (the entry shim's call; the state array lives in plaits.c). */
int plaits_track(int track, const PlaitsFrame *f, int trig, float *out, int n, int rate);

/* Clear a voice to its never-triggered state (silent until the next trigger). */
void plaits_voice_clear(PlaitsVoice *v);

/* Plaits' output stage on one channel of n samples in place (plaits.c; exported for the host test). */
void plaits_post(float *peak, float gain, float *x, int n);

/* The DC blocker over n samples in place (exported for the host test). */
void plaits_dc_block(PlaitsDcBlock *s, float *x, int n);

/* Output gain, and Plaits' per-model OUT and AUX gains as float bit patterns (negative: through the limiter): see
 * plaits.c. */
extern const float plaits_gain;
extern const unsigned int plaits_out_gain[24];
extern const unsigned int plaits_aux_gain[24];

#endif
