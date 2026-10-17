/* Host-only helper for tests/plaits/test_va_host.py: the sizes of the port's structs, so ctypes can allocate them.
 * Linked into the host library with the module's C sources; not part of the DSP object. */
#include "plaits.h"

int port_va_size(void) { return (int)sizeof(VA); }
int port_voice_size(void) { return (int)sizeof(PlaitsVoice); }
int port_frame_size(void) { return (int)sizeof(PlaitsFrame); }
