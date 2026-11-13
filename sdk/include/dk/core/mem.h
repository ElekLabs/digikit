/* <dk/core/mem.h>: memory of the module arena (dk_alloc, dk_free) and the firmware's own allocator and card read (dk_fw_alloc, dk_fw_free,
   dk_fw_block_read). Hand-written.

   `void *dk_alloc(uint32_t size)` gives a 16-byte aligned block from the module arena, or NULL when it is full; the block is not zeroed. `void dk_free(void *p)` gives it back.
   The arena is shared by all modules and the runtime (512 KiB): ask for what the module needs in `init`, and keep a block for the life of the
   module rather than allocating in `tick`. A module's static data (bss) comes from the same arena when it loads.

   `void *dk_fw_alloc(uint32_t size)` is the firmware's operator new (the 32 MiB buddy allocator, much larger than the arena): a block of `size` bytes or NULL;
   give it back with `dk_fw_free(p)`, never with `dk_free`. A module that needs a large buffer (a transfer staging buffer) takes it here and frees it when the
   work is done: the memory belongs to the firmware's samples and projects too. `int32_t dk_fw_block_read(uint32_t sector, uint32_t bytes, void *buf)` is the
   firmware's eMMC block read (0 = ok; `bytes` a multiple of 512, `buf` 16-byte aligned). It reads any sector of the card. */
#ifndef DK_MEM_H
#define DK_MEM_H
#include <dk/core/module.h>
#endif
