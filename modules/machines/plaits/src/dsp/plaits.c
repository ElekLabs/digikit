/* PLAITS for the Digitakt II: the voice wrapper (plaits.h). Plain C99, float only; host and Selache builds.
 *
 * Per Digitakt block (32 samples at 48 kHz):
 *   note      = pitch / 256 + (tune / 256 - 64)                 (the frame's note word and TUNE)
 *   harmonics, timbre, morph = word / 0x7f00, clamped to [0, 1]
 *   mix       = word / 0x7f00, clamped: out = OUT + mix (AUX - OUT)
 *   two engine renders of 16 samples (va_render for model 0, ws_render 1, fm_render 2, grain_render 3,
 *   additive_render 4, wavetable_render 5, chord_render 6, speech_render 7), then the DC blocker (one pole at 5 Hz)
 *   and the output gain.
 *   Plaits' trigger (Params.trigger) as Plaits' Voice builds it with TRIG patched and the gate held high from the
 *   note-on: RISING_EDGE | HIGH in the first sub-block of a note-on, HIGH in every other sub-block (see TRIG_*);
 *   accent 0.8, Plaits' value with LEVEL unpatched. Only SPEECH (7) reads them.
 * Velocity is not used by these models. The model is latched at a note-on; the engine is initialised at the
 * voice's first note-on and when a note-on changes the model, and otherwise runs on from note to note (note_on). A
 * model without an engine renders silence.
 */
#include "plaits.h"
#include "dsp_util.h"

/* 1 / 0x7f00: the full scale of the HARMONICS, TIMBRE, MORPH and MIX rows. */
#define PL_INV_FULL 3.0757874e-05f
#define PL_INV_256 0.00390625f

/* DC blocker y[n] = x[n] - x[n-1] + R y[n-1], R = 1 - 2 pi 5 / 48000 (pole at 5 Hz). */
#define PL_DC_R 0.99934550f

/* Output gain, measured in the emulator (tools/patchkit/pk_proof_plaits.py part g, DT2 1.17, 2026-10-17): on track 1 at
 * the default LEV, the default project's stock sample (int16 peak 0.49997, RMS at full scale 0.7071) gives an output
 * peak of 0.0737 per unit of sample peak, so a full-scale copy of it would give an output RMS of 0.0521; default VA
 * (HARMONICS, TIMBRE, MORPH 0x4000, MIX 0) at note 60 with gain 1 gives 0.0867 on the same track. 0.0521 / 0.0867 =
 * 0.601: default VA at note 60 then has the RMS of that full-scale stock sample at the same LEV (voice level: RMS 0.29,
 * peak 0.52). */
const float plaits_gain = 0.601f;

static float unit(float word)
{
    float x = word * PL_INV_FULL;
    CONSTRAIN(x, 0.0f, 1.0f);
    return x;
}

static void zeros(float *out, int n)
{
    int i;
    for (i = 0; i < n; i++) {
        out[i] = 0.0f;
    }
}

void plaits_dc_block(PlaitsDcBlock *s, float *x, int n)
{
    float x1 = s->x1;
    float y1 = s->y1;
    int i;
    for (i = 0; i < n; i++) {
        float in = x[i];
        y1 = in - x1 + PL_DC_R * y1;
        x1 = in;
        x[i] = y1;
    }
    s->x1 = x1;
    s->y1 = y1;
}

void plaits_voice_clear(PlaitsVoice *v)
{
    v->armed = 0;
    v->model = 0;
    v->dc.x1 = 0.0f;
    v->dc.y1 = 0.0f;
}

/* A note-on latches the model. The engine is (re)initialised only when the voice has no running engine yet (armed is
 * 0: the voice state is zero at load, plaits_voices below) or the latched model changes; every other note-on leaves
 * the engine, the DC blocker and the interpolator running, as Plaits keeps its engine across triggers (voice.cc: Init
 * at power-up, and at an engine change only LoadUserData and Reset). Here the engines share one union, so a model
 * change cannot resume the new engine's old state as Plaits does: it starts from Plaits' Init + LoadUserData(NULL) +
 * Reset (additive_init and chord_init include Reset, the other engines' Reset is empty; WAVETABLE's LoadUserData(NULL)
 * is the static wave map of lib/waves.c), with the DC blocker and the interpolator cleared, as Plaits resets its
 * post-processor. A note-on of the latched model still reaches the engine as Plaits' trigger (render_engine): SPEECH
 * starts its consonant or word there, as Plaits' engine does at a trigger without being re-initialised. */
static void note_on(PlaitsVoice *v, const PlaitsFrame *f)
{
    int model = (int)(f->model * PL_INV_256);
    if (v->armed && model == v->model) {
        return;
    }
    v->model = model;
    if (model == PLAITS_MODEL_ASYM_TRI) {
        ws_init(PL_WS(v));
    } else if (model == PLAITS_MODEL_2FM) {
        fm_init(PL_FM(v));
    } else if (model == PLAITS_MODEL_FORMANT) {
        grain_init(PL_GRAIN(v));
    } else if (model == PLAITS_MODEL_ADDITIVE) {
        additive_init(PL_ADD(v));
    } else if (model == PLAITS_MODEL_WAVETABLE) {
        wavetable_init(PL_WT(v));
    } else if (model == PLAITS_MODEL_CHORD) {
        chord_init(PL_CHORD(v));
    } else if (model == PLAITS_MODEL_SPEECH) {
        speech_init(PL_SPEECH(v));
    } else {
        va_init(PL_VA(v));
    }
    v->dc.x1 = 0.0f;
    v->dc.y1 = 0.0f;
    hb_init(&v->hb);
    v->armed = 1;
}

/* Plaits' trigger, as its Voice builds it with TRIG patched (plaits/dsp/voice.cc: (rising_edge ? RISING_EDGE : LOW)
 * | (gate high ? HIGH : LOW)). The Digitakt gives the voice routine the note-on only (the trigger latch), not the
 * note's end, so the gate is taken as high from a note-on to the next: RISING_EDGE | HIGH in the note-on's first
 * sub-block, HIGH after. No ported engine reads HIGH apart from the rising edge (in Plaits only the DX7 engines,
 * which are not ported, read it), so the choice of HIGH or LOW after the edge does not change SPEECH's output
 * (tests/plaits/test_speech_host.py); HIGH with the edge does: SPEECH's naive and SAM synths compare the trigger with
 * RISING_EDGE alone, which Plaits' patched trigger never equals. */
#define TRIG_RISING_HIGH 5
#define TRIG_HIGH 4

/* 32 samples at 48 kHz of the latched model (0..7) into dst; trig: a note-on in this block. */
static void render_engine(PlaitsVoice *v, const PlaitsFrame *f, int trig, float *dst)
{
    Params p;
    float mix;
    int s, i;

    p.note = f->pitch * PL_INV_256 + (f->tune * PL_INV_256 - 64.0f);
    p.harmonics = unit(f->harmonics);
    p.timbre = unit(f->timbre);
    p.morph = unit(f->morph);
    mix = unit(f->mix);
    p.accent = 0.8f;
    for (s = 0; s < PLAITS_BLOCK; s += PLAITS_SUB) {
        p.trigger = (trig && s == 0) ? TRIG_RISING_HIGH : TRIG_HIGH;
        if (v->model == PLAITS_MODEL_ASYM_TRI) {
            ws_render(PL_WS(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_2FM) {
            fm_render(PL_FM(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_FORMANT) {
            grain_render(PL_GRAIN(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_ADDITIVE) {
            additive_render(PL_ADD(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_WAVETABLE) {
            wavetable_render(PL_WT(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_CHORD) {
            chord_render(PL_CHORD(v), &p, v->out, v->aux, PLAITS_SUB);
        } else if (v->model == PLAITS_MODEL_SPEECH) {
            speech_render(PL_SPEECH(v), &p, v->out, v->aux, PLAITS_SUB);
        } else {
            va_render(PL_VA(v), &p, v->out, v->aux, PLAITS_SUB);
        }
        for (i = 0; i < PLAITS_SUB; i++) {
            float o = v->out[i];
            dst[s + i] = o + mix * (v->aux[i] - o);
        }
    }
    plaits_dc_block(&v->dc, dst, PLAITS_BLOCK);
    for (i = 0; i < PLAITS_BLOCK; i++) {
        dst[i] = dst[i] * plaits_gain;
    }
}

int plaits_render(PlaitsVoice *v, const PlaitsFrame *f, int trig, float *out, int n, int rate)
{
    float *dst;

    if (trig) {
        note_on(v, f);
    }
    if (!v->armed || !f->active || v->model < PLAITS_MODEL_VA || v->model > PLAITS_MODEL_SPEECH) {
        zeros(out, n);
        return 0;
    }
    dst = rate == 96000 ? v->buf48 : out;
    render_engine(v, f, trig, dst);
    if (rate == 96000) {
        hb_up2(&v->hb, v->buf48, out, PLAITS_BLOCK);
    }
    return 1;
}

/* The state of the 16 tracks' voices (the even voice of each track renders; the odd one copies it). Zero at load:
 * every voice starts never-triggered. */
static PlaitsVoice plaits_voices[16];

int plaits_track(int track, const PlaitsFrame *f, int trig, float *out, int n, int rate)
{
    return plaits_render(&plaits_voices[track & 15], f, trig, out, n, rate);
}
