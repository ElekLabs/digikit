/* Host-only helper for tests/plaits (test_va_host.py, test_ws_host.py, test_fm_grain_additive_host.py): the sizes of
 * the port's structs, so ctypes can allocate them, and the port's tables (waveshaping, lib). Linked into the host
 * library with the module's C sources; not part of the DSP object. */
#include "plaits.h"
#include "engines/ws_tables.h"
#include "engines/fm.h"
#include "engines/grain.h"
#include "engines/additive.h"
#include "lib/tables.h"

int port_va_size(void) { return (int)sizeof(VA); }
int port_ws_size(void) { return (int)sizeof(WS); }
int port_fm_size(void) { return (int)sizeof(FM); }
int port_grain_size(void) { return (int)sizeof(Grain); }
int port_additive_size(void) { return (int)sizeof(Additive); }
int port_engine_union_size(void) { return (int)sizeof(PlaitsEngine); }
int port_voice_size(void) { return (int)sizeof(PlaitsVoice); }
int port_frame_size(void) { return (int)sizeof(PlaitsFrame); }

/* Table k of ws_tables.h (gen_ws_tables.py order), entry i, as the port reads it; n = its length. */
static const unsigned int *const port_luts[8] = {
    lut_sine, lut_fold, lut_fold_2, lut_ws_inverse_tan, lut_ws_inverse_sin, lut_ws_linear, lut_ws_bump,
    lut_ws_double_bump,
};
static const int port_lut_sizes[8] = {513, 516, 516, 257, 257, 257, 257, 257};
float port_ws_lut(int k, int i) { return WS_F(port_luts[k])[i]; }
int port_ws_lut_size(int k) { return port_lut_sizes[k]; }

/* Table k of lib/tables.h (lib/gen_lib_tables.py order), entry i, as the port reads it; n = its length. */
static const unsigned int *const port_lib_luts[5] = {
    pl_lut_sine, pl_lut_fm_frequency_quantizer, pl_lut_4x_downsampler_fir, pl_lut_pitch_ratio_high,
    pl_lut_pitch_ratio_low,
};
static const int port_lib_lut_sizes[5] = {641, 130, 4, 256, 256};
float port_lib_lut(int k, int i) { return PL_F(port_lib_luts[k])[i]; }
int port_lib_lut_size(int k) { return port_lib_lut_sizes[k]; }
