/* Host-only helper for tests/plaits (test_va_host.py, test_ws_host.py, test_fm_grain_additive_host.py,
 * test_wavetable_chord_host.py, test_speech_host.py): the sizes of the port's structs, so ctypes can allocate them,
 * the port's tables (waveshaping, lib, waves, chord, speech) and the index recorder of a PL_INDEX_CHECK build. Linked into the host library with
 * the module's C sources; not part of the DSP object. */
#include "plaits.h"
#include "engines/ws_tables.h"
#include "engines/fm.h"
#include "engines/grain.h"
#include "engines/additive.h"
#include "engines/wavetable.h"
#include "engines/chord.h"
#include "lib/tables.h"
#include "lib/waves.h"
#include "engines/speech.h"
#include "lib/speech_tables.h"
#include "engines/modal.h"
#include "engines/terrain.h"

int port_va_size(void) { return (int)sizeof(VA); }
int port_ws_size(void) { return (int)sizeof(WS); }
int port_fm_size(void) { return (int)sizeof(FM); }
int port_grain_size(void) { return (int)sizeof(Grain); }
int port_additive_size(void) { return (int)sizeof(Additive); }
int port_wavetable_size(void) { return (int)sizeof(Wavetable); }
int port_chord_size(void) { return (int)sizeof(Chord); }
int port_speech_size(void) { return (int)sizeof(Speech); }
int port_modal_size(void) { return (int)sizeof(Modal); }
int port_terrain_size(void) { return (int)sizeof(Terrain); }
int port_engine_union_size(void) { return (int)sizeof(PlaitsEngine); }
int port_voice_size(void) { return (int)sizeof(PlaitsVoice); }
int port_frame_size(void) { return (int)sizeof(PlaitsFrame); }

/* Plaits' output gains of model n as the wrapper reads them (plaits.c). */
float port_out_gain(int n) { return PL_F(plaits_out_gain)[n]; }
float port_aux_gain(int n) { return PL_F(plaits_aux_gain)[n]; }

/* Table k of ws_tables.h (gen_ws_tables.py order), entry i, as the port reads it; n = its length. */
static const unsigned int *const port_luts[8] = {
    lut_sine, lut_fold, lut_fold_2, lut_ws_inverse_tan, lut_ws_inverse_sin, lut_ws_linear, lut_ws_bump,
    lut_ws_double_bump,
};
static const int port_lut_sizes[8] = {513, 516, 516, 257, 257, 257, 257, 257};
float port_ws_lut(int k, int i) { return WS_F(port_luts[k])[i]; }
int port_ws_lut_size(int k) { return port_lut_sizes[k]; }

/* Table k of lib/tables.h (lib/gen_lib_tables.py order), entry i, as the port reads it; n = its length. */
static const unsigned int *const port_lib_luts[6] = {
    pl_lut_sine, pl_lut_fm_frequency_quantizer, pl_lut_4x_downsampler_fir, pl_lut_pitch_ratio_high,
    pl_lut_pitch_ratio_low, pl_lut_stiffness,
};
static const int port_lib_lut_sizes[6] = {641, 130, 4, 256, 256, 65};
float port_lib_lut(int k, int i) { return PL_F(port_lib_luts[k])[i]; }
int port_lib_lut_size(int k) { return port_lib_lut_sizes[k]; }

/* The wave table and the wave map of lib/waves.h, as the port reads them. */
float port_wave(int i) { return PL_F(pl_wav_integrated_waves)[i]; }
int port_wave_count(void) { return (int)(sizeof(pl_wav_integrated_waves) / sizeof(pl_wav_integrated_waves[0])); }
int port_wave_map(int i) { return pl_wavetable_map[i]; }

/* ChordBank's ratios (chord i, note j) and note counts, through lib/chord_bank.h with the chord selected the way
 * upstream selects it (set_chord). */
float port_chord_ratio(float parameter, int note) {
    ChordBank b;
    chord_bank_init(&b);
    chord_bank_reset(&b);
    chord_bank_set_chord(&b, parameter);
    return chord_bank_ratio(&b, note);
}
int port_chord_num_notes(float parameter) {
    ChordBank b;
    chord_bank_init(&b);
    chord_bank_set_chord(&b, parameter);
    return chord_bank_num_notes(&b);
}
int port_chord_inversion(float parameter, float inversion, float *ratios, float *amplitudes, int *index) {
    ChordBank b;
    chord_bank_init(&b);
    chord_bank_reset(&b);
    chord_bank_set_chord(&b, parameter);
    *index = chord_bank_chord_index(&b);
    return chord_bank_compute_chord_inversion(&b, inversion, ratios, amplitudes);
}

/* SPEECH's tables as the port reads them: the naive phonemes (phoneme p, register r, formant i; k 0 frequency, 1
 * amplitude), the SAM phonemes (p < 18, formant i; k 0 frequency, 1 amplitude), the LPC vowels and consonants (frame p,
 * field k of PL_LPC_FRAME), a word bank's frame (bank b, frame i) as LPCFrame floats (12: 1 / period, energy, k0..k9),
 * the banks' frame and word counts and boundaries, and the excitation pulse. */
float port_naive_phoneme(int p, int r, int i, int k) { return PL_F(pl_naive_phonemes)[p * 50 + r * 10 + i * 2 + k]; }
float port_sam_phoneme(int p, int i, int k) { return PL_F(pl_sam_phonemes)[p * 6 + i * 2 + k]; }
float port_lpc_phoneme(int p, int k) { return PL_F(pl_lpc_phonemes)[p * PL_LPC_FRAME + k]; }
void port_lpc_word_frame(int b, int i, float *out) {
    LPCFrame f;
    int j;
    lpc_word_frame(b, i, &f);
    out[0] = f.inv_period;
    out[1] = f.energy;
    for (j = 0; j < 10; j++) out[2 + j] = f.k[j];
}
int port_lpc_bank_num_frames(int b) { return pl_lpc_bank_num_frames[b]; }
int port_lpc_bank_num_words(int b) { return pl_lpc_bank_num_words[b]; }
int port_lpc_bank_boundary(int b, int i) { return pl_lpc_word_boundaries[pl_lpc_bank_boundary0[b] + i]; }
float port_lpc_excitation_pulse(int i) { return PL_F(pl_lpc_excitation_pulse)[i]; }

/* The index recorder of a PL_INDEX_CHECK build (the engines call pl_index_check(table, index) before each table
 * read): per table, the smallest and largest index seen and the count. Tables: 0 the WAVETABLE wave map index,
 * 1 a WAVETABLE sample index within a wave (the last of the four Hermite reads), 2 a wavetable oscillator's wave
 * index (CHORD's wave list), 3 its sample index, 4 CHORD's registration row, 5 ChordBank's ratio index, 6 a chord
 * voice index; SPEECH (lib/naive_speech_synth.c, sam_speech_synth.c, lpc_speech_synth.c): 7 the naive phoneme row, 8 its
 * register row, 9 the SAM phoneme, 10 a SAM formant's sine index, 11 the LPC excitation pulse index, 12 an LPC vowel
 * or consonant frame, 13 a word bank frame, 14 the loaded bank's num_frames - 1 - that frame, 15 the word, 16 the word
 * bank, 17 num_words - (word + 1); RINGS (lib/resonator.c): 18 the lut_stiffness index; TERRAIN (engines/terrain.c):
 * 19 a Sine phase * 65536 (-1: negative), 20 a wavetable terrain's wave coordinate (int)wt (-1: negative), 21 its
 * sample coordinate (int)sample (-1: negative), 22 the last wave-table entry it reads, 23 the terrain index, 24 the
 * path's x * 10000, 25 its y * 10000, 26 the first entry of each three-entry wave read, 27 a sine-table index below 0. */
#define PL_INDEX_TABLES 32
static int idx_lo[PL_INDEX_TABLES], idx_hi[PL_INDEX_TABLES], idx_n[PL_INDEX_TABLES];
void pl_index_check(int table, int index) {
    if (idx_n[table] == 0 || index < idx_lo[table]) idx_lo[table] = index;
    if (idx_n[table] == 0 || index > idx_hi[table]) idx_hi[table] = index;
    idx_n[table]++;
}
void port_index_reset(void) {
    int i;
    for (i = 0; i < PL_INDEX_TABLES; i++) idx_n[i] = 0;
}
int port_index_lo(int table) { return idx_lo[table]; }
int port_index_hi(int table) { return idx_hi[table]; }
int port_index_n(int table) { return idx_n[table]; }

