#include "dkrt.h"

extern int32_t dk_peer_audio_load(const uint8_t *path);
extern int32_t dk_peer_audio_trigger(uint32_t generation, uint32_t gain);
extern int32_t dk_peer_audio_select_track(uint32_t track);
extern int32_t dk_peer_audio_status(void);

volatile int32_t peer_audio_proof_result;

/* panel_drive invokes this at the stock UI-loop hook. */
void peer_audio_proof(const uint8_t *bytes, uint32_t length)
{
    peer_audio_proof_result = DKRT_E_FORMAT;
    if (!length)
        return;
    if (bytes[0] == 'P' && length > 2 && bytes[length - 1] == 0)
        peer_audio_proof_result = dk_peer_audio_load(bytes + 1);
    else if (bytes[0] == 'T' && length == 1)
        peer_audio_proof_result = dk_peer_audio_trigger(0, 256);
    else if (bytes[0] == 'A' && length == 2)
        peer_audio_proof_result = dk_peer_audio_select_track(bytes[1]);
    else if (bytes[0] == 'S' && length == 1)
        peer_audio_proof_result = dk_peer_audio_status();
}
