/* The layout of <dk/host/voice_host.h> as the header comment states it, for any pointer size P (4 on the ColdFire). The Rust mirror
   (runtime/core/sdk/src/voice_host/tests.rs) asserts the same formulas. */
#include <dk/host/voice_host.h>
#include <stddef.h>

#define P sizeof(void *)
#define ALIGN(n) (((n) + P - 1) / P * P)
#define DESC ALIGN(P + 4)

_Static_assert(offsetof(dk_voice_install, payload_bytes) == P, "payload_bytes");
_Static_assert(offsetof(dk_voice_install, desc) == DESC, "desc");
_Static_assert(offsetof(dk_voice_install, range_min) == DESC + P, "range_min");
_Static_assert(offsetof(dk_voice_install, range_max) == DESC + P + 32, "range_max");
_Static_assert(offsetof(dk_voice_install, cost) == DESC + P + 64, "cost");
_Static_assert(sizeof(dk_voice_install) == ALIGN(DESC + P + 68), "install size");
_Static_assert(sizeof(dk_voice_stats) == 24, "stats size");
_Static_assert(sizeof(dk_machine_desc) == 32, "desc size");

_Static_assert(offsetof(dk_voice_host_v1, major) == 4, "major");
_Static_assert(offsetof(dk_voice_host_v1, minor) == 6, "minor");
_Static_assert(offsetof(dk_voice_host_v1, size) == 8, "size");
_Static_assert(offsetof(dk_voice_host_v1, tracks) == 12, "tracks");
_Static_assert(offsetof(dk_voice_host_v1, types_max) == 14, "types_max");
_Static_assert(offsetof(dk_voice_host_v1, payload_bytes_max) == 16, "payload_bytes_max");
_Static_assert(offsetof(dk_voice_host_v1, state_bytes_max) == 20, "state_bytes_max");
_Static_assert(offsetof(dk_voice_host_v1, budget) == 24, "budget");
_Static_assert(offsetof(dk_voice_host_v1, flags) == 28, "flags");
_Static_assert(offsetof(dk_voice_host_v1, ctx) == 32, "ctx");
_Static_assert(offsetof(dk_voice_host_v1, install) == 32 + P, "install");
_Static_assert(offsetof(dk_voice_host_v1, bind) == 32 + 2 * P, "bind");
_Static_assert(offsetof(dk_voice_host_v1, retire) == 32 + 3 * P, "retire");
_Static_assert(offsetof(dk_voice_host_v1, stats) == 32 + 4 * P, "stats");
_Static_assert(sizeof(dk_voice_host_v1) == 32 + 5 * P, "table size");

int main(void)
{
    return 0;
}
