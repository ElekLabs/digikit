/* Host-only helper for tests/plaits (test_va_host.py, test_ws_host.py): the sizes of the port's structs, so ctypes
 * can allocate them, and the port's waveshaping tables. Linked into the host library with the module's C sources;
 * not part of the DSP object. */
#include "plaits.h"
#include "engines/ws_tables.h"

int port_va_size(void) { return (int)sizeof(VA); }
int port_ws_size(void) { return (int)sizeof(WS); }
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
