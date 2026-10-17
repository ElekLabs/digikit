/* PLAITS for the Digitakt II: the voice wrapper around the Plaits engines (plain C99, float only).
 *
 * Built twice from the same source: on the host by tests/plaits (ctypes) and by Selache's C compiler into the DSP
 * object of machines/plaits-48k and machines/plaits-96k, where src/dsp/plaits_entry.s calls plaits_render from the
 * core/dsp-inline voice hook. Pointers are byte addresses on the SHARC+ (Selache's C), as are the Digitakt's buffers.
 */
#ifndef PLAITS_PLAITS_H
#define PLAITS_PLAITS_H

#include "engines/va.h"
#include "halfband.h"

/* Plaits sub-block: two per Digitakt block of 32 samples at 48 kHz (Plaits' own control rate is 4 kHz, here 3 kHz). */
#define PLAITS_SUB 16
#define PLAITS_BLOCK 32

/* Model numbers are permanent (docs/design/2026-10-17-plaits-machine-design.md): VA = 0. */
#define PLAITS_MODEL_VA 0

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
    int active;      /* the voice record's ACTIVE byte (+0x1b8) */
} PlaitsFrame;

/* The one-pole DC blocker at the voice output (about 5 Hz). */
typedef struct {
    float x1, y1;
} PlaitsDcBlock;

/* Per-voice state. */
typedef struct {
    VA va;
    float out[PLAITS_SUB]; /* the engine's OUT and AUX for one sub-block */
    float aux[PLAITS_SUB];
    PlaitsDcBlock dc;
    int model;             /* latched at note-on */
    int armed;             /* triggered (and the engine initialised) since the type reached this voice */
    HalfBand hb;           /* the 2x interpolator, used when the output rate is 96 kHz */
    float buf48[PLAITS_BLOCK];
} PlaitsVoice;

/* Render one Digitakt block into `out`: n = 32 samples at rate 48000 (written as they are), or n = 64 at 96000 (32
 * rendered at 48 kHz, then hb_up2). `trig` is the voice's trigger-latch byte (non-zero in the block of a note-on):
 * it latches the model and restarts the engine. A voice renders only when it was triggered since its state was
 * last cleared and its ACTIVE byte is set, and only model VA (0) has an engine yet; otherwise the block is zeros.
 * Returns 1 when it rendered the engine, else 0. */
int plaits_render(PlaitsVoice *v, const PlaitsFrame *f, int trig, float *out, int n, int rate);

/* plaits_render for the voice state of track 0..15 (the entry shim's call; the state array lives in plaits.c). */
int plaits_track(int track, const PlaitsFrame *f, int trig, float *out, int n, int rate);

/* Clear a voice to its never-triggered state (silent until the next trigger). */
void plaits_voice_clear(PlaitsVoice *v);

/* The DC blocker over n samples in place (exported for the host test). */
void plaits_dc_block(PlaitsDcBlock *s, float *x, int n);

/* Output gain: see plaits.c. */
extern const float plaits_gain;

#endif
