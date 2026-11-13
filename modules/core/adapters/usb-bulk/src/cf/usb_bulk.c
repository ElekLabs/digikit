/* core/usb-bulk: a vendor bulk interface (interface 2: EP2 OUT and EP2 IN, 512-byte packets) added to the firmware's USB MIDI mode.
 * Plan: docs/plans/usb-fast.md section 4. Facts: docs/findings/42-usb-transfer-routes.md sections 1 and 4 (static, DT2 1.17 and DN2 1.12).
 *
 * The USB engine is the firmware's and is not patched. Its port table is RAM: entry 2 (USB MIDI) holds the connect and disconnect callbacks and the
 * pointer and length of the configuration descriptor. usb_bulk_enable repoints those four words (a 124-byte copy of the stock 101-byte descriptor
 * with interface 2 appended; callbacks of ours that call the stock ones) and re-enumerates through the firmware's own stop and restart pair.
 *
 * RX: a ring of up to RX_MAX slots of RX_BYTES (RX_PAGES 4 KiB pages: the firmware only builds single-page dTDs; ours name all five page pointers, see queue_transfer). Each is queued
 * as one dTD on EP2 OUT. A dTD completes when it is full or when a packet shorter than 512 bytes (a zero-length packet too) ends the transfer: the host
 * sends a zero-length packet after any frame whose length is a multiple of 512 bytes. The completion callback runs in the USB interrupt: it marks the buffer
 * full and queues a free one. With no free buffer nothing is queued, the host gets NAK, and that is the whole flow control. The dTDs come from the firmware's
 * pool (the engine's own EP1 callback does the same from the interrupt); its free callback is wrapped to see the status byte, and a halted or error status
 * drops the transfer. TX: usb_bulk_send copies into one of TX_SLOTS slots and queues one dTD on EP2 IN (64-byte packets, see QH_CAPS_IN); the completion frees it.
 *
 * Buffers are accessed through the uncached alias of DDR, so the DMA and the CPU never see stale cache lines; only the receive reads of a module use the
 * cached address, after an invalidate (UB_CACHED_RX). The stock disconnect callback polls
 * ENDPTFLUSH without a bound; ours flushes EP2 first with a bounded poll. Interrupts are masked (SR 0x2700) around every state change, never around a
 * copy: the state is touched by the I/O task, the UI loop tick and the USB interrupt. A refused dTD leaves `need_prime`, retried from the module's calls
 * and from usb_bulk_tick (dkrt_poll_add).
 *
 * Take-over (docs/plans/usb-fast-takeover.md): usb_bulk_takeover switches the device from USB AUDIO/MIDI (port 5) to USB MIDI (port 2) in RAM with the firmware's own
 * mode switch, and usb_bulk_giveback (or an automatic give-back) switches back. The stored USB CONFIG setting byte is never written, so a reboot always returns
 * to the stored mode. The calls only record a request; the switch runs from usb_bulk_tick (UI task) at least TO_DELAY_MS later, so a SysEx reply is out first and the
 * switch never runs in the MIDI task or an interrupt. The tick is on the runtime's list from the first takeover call on (bulk need not be enabled).
 * One switch at a time: the firmware's switch has no lock and restarts the device some ticks after it writes the port. Every switch, and enable's own stop and restart,
 * stamps a guard (sw_stamp); no other switch runs, enable returns E_STATE and our disconnect callback records no host leaving until TO_GUARD_MS have passed. The tick
 * claims a pending request under the interrupt mask before it calls the switch (to_pend_port), so a takeover() or giveback() on the MIDI task at that moment is
 * a follow-up request that waits for the guard.
 *
 * Memory: the descriptor is static; the receive and send buffers are one block from the firmware's allocator (buffers_alloc), allocated when the interface can go
 * live and freed when the take-over ends (buffers_free). EP2 stays enabled (ENDPTCTRL2) from the host's connect until the disconnect, whether or not the buffers exist:
 * without them the host gets NAK. usb_bulk_connect always sets the endpoint up and queues the buffers only when they exist; buffers_alloc queues them when the
 * endpoint is up already; buffers_free takes the transfers back (flush, cancel) and frees only when the controller reports EP2 idle.
 *
 * Contexts: usb_bulk_connect and usb_bulk_disconnect are called by the firmware (the USB interrupt, the mode switch); usb_bulk_rx_done, usb_bulk_tx_done
 * and usb_bulk_cancel are the dTD callbacks (interrupt, or the cancel path of the disconnect). The others are for modules (usb_bulk_* of sdk.toml),
 * from the I/O task. */

#define UB_TRACE 0 /* 1 for hardware debug builds: log calls */

#include <stdint.h>

/* The opaque handle types of the service imports (sdk/sdk.toml [handle]); <dk/gen/imports.h> is not included: it also declares the old import names with the SDK's integer types. */
typedef struct dk_module_ctx dk_module_ctx;
typedef struct dk_usb_bulk dk_usb_bulk;

typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char u8;
typedef int i32;

/* sdk.toml [load.status] */
#define E_NOMEM (-8)
#define E_BUSY (-12)
#define E_CONTRACT (-48)
#define E_STATE (-53)
#define E_LENGTH (-55)
/* adapter status (<dk/platform/usb_bulk.h>): nothing is in flight and the queue refused; the adapter's tick wakes the job to try again */
#define E_AGAIN (-72)

/* what a module sees: <dk/platform/usb_bulk.h> */
#define ST_ENABLED 0x1u
#define ST_CONNECTED 0x2u
#define ST_WRONG_MODE                                                                              \
    0x4u /* the active USB port has no bulk route: not USB MIDI, and not USB AUDIO/MIDI with core/usb-bulk-audio (live) */
#define ST_TAKEN 0x8u /* a take-over is active (requested, or the device is switched) */
#define ST_LIVE                                                                                    \
    0x10u /* the buffers are allocated (from the moment the device goes to, or is in, USB MIDI with bulk, until the take-over ends) */
#define ST_NOMEM 0x20u /* the last allocation of the buffers failed (cleared by the next success) */
#define ST_OUT_ONLY                                                                                \
    0x40u /* USB AUDIO/MIDI with core/usb-bulk-audio is the active port: bulk OUT only, no bulk IN (live) */

/* ---- the geometry ---- */
#define PAGE 4096u
/* The receive ring: slots of 16 KiB. The block is allocated with the largest tier the heap can give (RING_TIERS): 60 slots (960 KiB, a 1 MiB block), 28 (a 512 KiB
 * block) or 8 (128 KiB, a 256 KiB block: the old geometry). Only RX_INFLIGHT dTDs are queued on the controller at once, from the firmware's pool of 31 (finding 42):
 * 12 RX + 2 TX = 14, leaving 17 for the firmware's own (EP0, EP1 MIDI: a few). The interrupt queues the next free slot whenever one completes, so the controller keeps
 * receiving into free slots while the I/O task writes the card; the task takes the full slots in arrival order. With the ring switch off only RX_OLD (8) slots are used
 * and all of them are queued, exactly the old behaviour. */
#define RX_MAX 60u /* slots of the largest tier: state arrays are this long */
#define RX_OLD 8u
#define RX_INFLIGHT 12u
#define RX_PAGES 4u
#define RX_BYTES (RX_PAGES * PAGE)
#define TX_SLOTS 2u
#define TX_PAGES 4u
#define TX_BYTES (TX_PAGES * PAGE)
#define DESC_BYTES 124u

/* ---- the MCF5441x USB controller and the firmware's queue heads (the engine's own constants, FUN_400043e2) ---- */
#define USB_REG(off) (*(volatile u32 *)(0xfc0b0000u + (off))) /* writes */
/* Reads: always one 32-bit move. m68k GCC folds `USB_REG(x) & small_mask` into a btst on one byte (a byte access), and a register model that answers only long reads
 * (the emulator did) then sees stale bytes. The asm cannot be refolded. */
static inline __attribute__((always_inline)) u32 reg_rd(u32 off)
{
    u32 v;
    __asm__ volatile("move.l %1,%0" : "=d"(v) : "m"(*(volatile u32 *)(0xfc0b0000u + off)));
    return v;
}
#define USBCMD 0x140u
#define USBSTS 0x144u
#define FRINDEX 0x14cu
#define DEVICEADDR 0x154u
#define PORTSC 0x184u
#define PORTSC_SUSP 0x80u /* bit 7: the port is suspended */
#define ENDPTPRIME 0x1b0u
#define ENDPTCOMPLETE 0x1bcu
#define ENDPTSTAT 0x1b8u
#define ENDPTFLUSH 0x1b4u
#define ENDPTCTRL1 0x1c4u
#define ENDPTCTRL2 0x1c8u
#define ENDPTCTRL3 0x1ccu
#define USBCMD_RS 0x1u
#define OTGSC 0x1a4u
#define OTGSC_BSV                                                                                  \
    0x800u /* bit 11: B session valid (VBUS present); the firmware's restart waits for it */
#define ULPI_VIEWPORT 0x170u
#define ULPI_WU 0x80000000u  /* bit 31: wake-up (clears itself) */
#define ULPI_RUN 0x40000000u /* bit 30: a read or write is running (the controller clears it) */
#define ULPI_RW 0x20000000u  /* bit 29: 1 write */
#define ULPI_SS                                                                                    \
    0x08000000u /* bit 27: the ULPI interface is in normal sync state; a run is undefined without it */
#define GPTIMER0CTL                                                                                \
    0x84u /* USB general purpose timer 0: bits 23:0 count down in microseconds on the USB clock */
#define ULPI_FUNC_CTRL 0x04u
/* ULPI function control: XcvrSelect 01 (full speed), TermSelect 0 (no pull-up), OpMode 01 (non-driving), Reset 0, SuspendM 1 */
#define ULPI_FC_DETACH 0x49u
#define ULPI_SPINS 1000u
#define EP_IN_CTRL_MIDI                                                                            \
    0x00030000u /* ENDPTSTAT / ENDPTPRIME bits of the IN endpoints 0 and 1: a reply is on its way to the host */
#define FLUSH_EP2 ((1u << 2) | (1u << 18)) /* RX bit 2, TX bit 16 + 2 */
#define FLUSH_SPINS 20000u
#define QH_BASE 0x80000000u
#define QH_EP2_OUT 0x100u /* queue index 2 = endpoint + 4 * direction */
#define QH_EP2_IN 0x140u  /* queue index 6 */
#define IDX_OUT 2u
#define IDX_IN 6u
#define QH_CAPS_OUT 0x22000000u /* 512-byte packets, as the firmware sets EP1 OUT */
/* 64-byte packets, zero-length termination off, as the firmware sets EP1 IN. The TX buffer holds 64 bytes per endpoint; two back-to-back 512-byte IN
 * packets wedged the controller on the DN2. Each 64-byte packet is a short packet for the host, so no zero-length packet is needed. The descriptor
 * still says 512 (USB 2.0 requires it for high-speed bulk). */
#define QH_CAPS_IN 0x20400000u
#define TD_END 0xdead0001u
#define TD_ACTIVE 0x80u
#define TD_HALTED                                                                                  \
    0x40u /* status bits of a finished dTD: halted, data buffer error, transaction error */
#define TD_BUFERR 0x20u
#define TD_XACTERR 0x08u
#define CTRL_BULK                                                                                  \
    0x00c800c8u /* enable, data toggle reset, bulk, both directions: the value the firmware writes to ENDPTCTRL1 */
/* USB AUDIO/MIDI (port 5): EP2 OUT only. EP2 IN is the audio clock feedback of the firmware: never touched there. */
#define FLUSH_EP2_RX (1u << 2) /* ENDPTFLUSH, ENDPTSTAT, ENDPTPRIME, ENDPTCOMPLETE bit of EP2 OUT */
#define CTRL_RX_MASK 0x0000ffffu /* RXE, RXR, RXT and the rest of the receive half of ENDPTCTRL2 */
#define CTRL_RX_BULK                                                                               \
    0x000000c8u /* RXE (bit 7), RXR data toggle reset (bit 6), RXT bulk (bits 3:2 = 2) */

/* the port table entry of USB MIDI (docs/findings/42 section 4) */
#define PORT_ENTRY 2u
#define PORT_AUDIO 5u /* USB AUDIO/MIDI */
#define PORT_OVERBRIDGE 6u
#define PORT_ENTRY_BYTES 0x28u
#define STOCK_CONFIG_BYTES 101u

/* firmware (anchors by hash in the port files) */
extern u8 usb_port_table[];
extern u8 usb_active_port[]; /* u32 in RAM: 2 USB MIDI, 5 USB AUDIO/MIDI, 6 Overbridge */
extern void usb_fw_connect_body(u32 hs);
extern void usb_fw_disconnect(void);
extern i32 usb_fw_queue_td(u32 index, u8 *dtd);
extern void usb_fw_cancel_ep(u32 index);
extern u8 *usb_fw_td_alloc(void);
extern void usb_fw_stop(void);
extern void usb_fw_reset_stop(
    void); /* the firmware's stop with restart type 2: after 30 ticks its tick resets the controller (USBCMD.RST), initialises it and starts it if VBUS is present. Interrupts masked; never usb_fw_restart after it. */
extern void usb_fw_restart(void);
extern u8
    usb_restart_delay[]; /* u32 in RAM: 30 down to 0, non-zero while a stop waits to restart */
extern u8 *usb_fw_mem_new(
    u32 size); /* the firmware's operator new (the 32 MiB buddy allocator) and its free */
extern void usb_fw_mem_free(u8 *p);
extern void usb_fw_mode_switch(
    u32 port); /* FUN_40006c16: returns when `port` is active; else stop, write the port, restart after about 30 USB ticks. Task context. */
/* the board: the uncached alias of DDR is this far above the cached window (device.s) */
extern const u32 usb_alias_delta;
extern const u32 usb_ddr_base; /* the cached DDR window: base and size */
extern const u32 usb_ddr_bytes;
/* the runtime (dkrt.h) */
extern void dkrt_io_wake(i32 id);
extern i32 dkrt_poll_add(u32 f); /* the UI loop calls f once a pass */
extern u32 dkrt_blocks(void);    /* DSP blocks since boot (1500 a second); 0 without core/health */
extern void dkrt_log_ev(u32 kind, u32 flags, const u32 *a);

/* sdk.toml [[log.kind]] 14 MOD_LOG: args msg0..msg3 (16 ASCII chars, big-endian words) and value */
#define UB_KIND_MOD_LOG 14u
/* flags: not an error (bit 0 clear), level 3 (debug) in bits 1-3 (sdk.toml level_shift 1) */
#define UB_FLAGS (3u << 1)
/* UB_NOTE: level 1 (warn) */
#define UB_FLAGS_NOTE (1u << 1)

/* `tag`: a string of at most 16 chars, padded with NULs */
static void ub_log(const char *tag, u32 value, u32 flags)
{
    u32 a[5];
    u32 i;
    u32 live = 1;
    for (i = 0; i < 16u; i++) {
        u32 c = 0;
        if (live && tag[i] != 0)
            c = (u8)tag[i];
        else
            live = 0;
        if ((i & 3u) == 0u)
            a[i >> 2] = 0;
        a[i >> 2] |= c << (24u - 8u * (i & 3u));
    }
    a[4] = value;
    dkrt_log_ev(UB_KIND_MOD_LOG, flags, a);
}
/* UB_LOG: trace builds only. UB_NOTE: always, for the rare events that a hardware run must be able to see (a forced restart). UI task only. */
#if UB_TRACE
#define UB_LOG(tag, v) ub_log((tag), (v), UB_FLAGS)
#else
#define UB_LOG(tag, v) ((void)0)
#endif
#define UB_NOTE(tag, v) ub_log((tag), (v), UB_FLAGS_NOTE)

/* the stock configuration descriptor with wTotalLength (bytes 2 and 3, written by the firmware at run time) left out, and the interface appended */
static const u8 stock_config[STOCK_CONFIG_BYTES] = {
    0x09, 0x02, 0x00, 0x00, 0x02, 0x01, 0x00, 0xc0, 0x03, 0x09, 0x04, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x00, 0x00, 0x09, 0x24, 0x01, 0x00, 0x01, 0x09, 0x00, 0x01, 0x01, 0x09, 0x04, 0x01,
    0x00, 0x02, 0x01, 0x03, 0x00, 0x00, 0x07, 0x24, 0x01, 0x00, 0x01, 0x41, 0x00, 0x06, 0x24,
    0x02, 0x01, 0x01, 0x00, 0x06, 0x24, 0x02, 0x02, 0x02, 0x00, 0x09, 0x24, 0x03, 0x01, 0x03,
    0x01, 0x02, 0x01, 0x00, 0x09, 0x24, 0x03, 0x02, 0x04, 0x01, 0x01, 0x01, 0x00, 0x09, 0x05,
    0x01, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x05, 0x25, 0x01, 0x01, 0x01, 0x09, 0x05, 0x81,
    0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x05, 0x25, 0x01, 0x01, 0x03};
static const u8 bulk_interface[23] = {
    0x09, 0x04, 0x02, 0x00, 0x02, 0xff, 0x00,
    0x00, 0x00,                                /* interface 2, vendor class, two endpoints */
    0x07, 0x05, 0x02, 0x02, 0x00, 0x02, 0x00,  /* EP2 OUT, bulk, 512 */
    0x07, 0x05, 0x82, 0x02, 0x00, 0x02, 0x00}; /* EP2 IN, bulk, 512 */

/* ---- state ---- */
enum { B_FREE = 0, B_PRIMED, B_FULL, B_TAKEN };
enum { T_FREE = 0, T_RESERVED, T_QUEUED };

/* The descriptor stays for good (the port table entry points at it until the next boot). The receive and send buffers are allocated while the interface can be
 * live (buffers_alloc) and freed when the take-over ends (buffers_free); between, the adapter holds only this descriptor. */
static u8 desc_mem[DESC_BYTES + 16] __attribute__((aligned(16)));
static u8 *rx_area,
    *tx_area; /* the 4 KiB aligned buffers, as the CPU sees them (cached address); null while not live */
static u8 *blk_raw; /* what the allocator returned (freed as it is) */
static volatile u32 live, alloc_busy, free_req, nomem;
/* for the emulator test: the last block allocated (kept after the free) */
volatile u32 usb_bulk_last_area, usb_bulk_last_bytes;
volatile u32
    usb_bulk_free_why; /* why the last buffers_free refused: 1 a receive buffer is held, 2 a send slot is reserved, 4 EP2 not idle (the flush, ENDPTSTAT and ENDPTPRIME bits are in bits 8 and up), 8 a transfer of ours is queued, 16 allocating; 0 freed */
static volatile u8 rx_state[RX_MAX];
static u32 rx_len[RX_MAX];
static u8 order[RX_MAX]; /* the full slots in arrival order */
static u32 order_head, order_count;
/* The ring. `rx_slots`: the slots of the block allocated for this session (0 while not live). `usb_bulk_ring_off`: the run-time switch (bit 1 of the usb-fast switch mask; on by default):
 * off = only RX_OLD slots are used and all are queued, the old geometry. It is read at every queueing and at the next allocation (a block is sized by it). `prime_cur`:
 * where the interrupt looks for the next free slot, so the slots are used round the ring. */
static u32 rx_slots, prime_cur;
volatile u32
    usb_bulk_ring_off; /* 1 = the switch is off (0, the default, is on: bss); also the emulator test's handle on it */
volatile u32
    usb_bulk_tier_cap; /* emulator test knob: a tier of more slots than this (when not 0) counts as an allocation that failed */
volatile u32
    usb_bulk_last_slots; /* for the emulator test: the slots of the last block allocated (kept after the free) */
static volatile u8 tx_state[TX_SLOTS];
static volatile u32 enabled, connected, resets;
/* `stopping`: ep2_stop is cancelling the queues; the callbacks it runs must not queue anything. `need_prime`: a receive buffer is free and the
 * engine refused its dTD (the pool was empty); the tick retries. `tx_retry`: a send was refused; the tick wakes the job. `qh_ready`: the EP2 queue
 * heads were set up by this adapter. */
static volatile u32 stopping, need_prime, tx_retry, qh_ready;
static u32
    restart_due; /* enable_prepare found the device in USB MIDI mode: the first enable re-enumerates */
static volatile i32 io_job;
/* Take-over state. `taken`: a take-over is active. `to_req`: 0 none, 1 the switch to port 2 is requested, 2 the switch back is requested; the tick runs it when
 * `to_due_ms` have passed since it first saw the request. `to_orig`: the port to give back to. `to_active`: set by any transfer or renew (interrupt or task),
 * consumed by the tick, which stamps `to_act_at`. `to_gone`: the disconnect callback ran while taken over. `to_quiet`: usb_bulk_enable is re-enumerating
 * (its own stop runs our disconnect callback). Times are in clock units of the tick (blocks, or tick passes without a block clock; `to_per_s` per second). */
static volatile u32 taken, to_req, to_active, to_gone, to_quiet;
static u32 to_orig, to_due_ms, to_due_set, to_due_at, to_act_at, to_sus, to_sus_at, to_per_s,
    to_passes;
/* The switch guard. The firmware's mode switch has no lock: it writes the active port at once and restarts the device about 30 USB ticks later, and a second
 * stop or restart in that window is not safe. `to_sw_set`: a switch (the firmware's mode switch, or enable's own stop and restart) was made at `to_sw_at`
 * (clock units of `to_sw_per` a second). No further switch runs, and our disconnect callback records no host leaving, until TO_GUARD_MS have passed. All under
 * the interrupt mask. `to_pend_port`: the target of a switch the tick has claimed and not yet written (0: none), so takeover() sees the port the switch will leave. */
static u32 to_sw_set, to_sw_at, to_sw_per;
/* `rec_armed`: a switch of ours was made and the controller has not been seen running since (recovery_tick). `rec_at`, `rec_per`: when it was made, or when a restart was last forced. */
static volatile u32 rec_armed;
static u32 rec_at, rec_per, rec_target,
    rec_count; /* `rec_target`: the port our switch went to; `rec_count`: restarts forced for this switch */
static volatile u32 to_pend_port;
/* The switch timeline (USB_SW events, "the switch timeline" below). `sw_open`: a timeline window is open. `sw_pend`: a request was accepted (1 take-over, 2 release); the tick logs REQ.
 * The two callbacks of the firmware run in the USB interrupt and only count, stamp the block clock and copy PORTSC; the tick logs them. */
static volatile u32 sw_open, sw_pend, sw_drop, sw_con_cnt, sw_con_at, sw_con_ps, sw_dis_cnt,
    sw_dis_at, sw_dis_ps;
static void sw_cb(volatile u32 *cnt, volatile u32 *at, volatile u32 *ps)
{
    (*cnt)++;
    if (sw_open) {
        *at = dkrt_blocks();
        *ps = reg_rd(PORTSC);
    }
}
static void (*stock_connect)(u32);
static u32 stock_free; /* the firmware's own free callback of a dTD (+0x38), which ours wraps */
static volatile u32
    td_status; /* the status byte of the dTD whose free callback ran last (read by the completion callback that follows) */
/* Counters for the emulator test (read through the plan's symbols): the mode switches and restarts made so far, and the time between the last two in ms. */
volatile u32 usb_bulk_switches, usb_bulk_switch_gap_ms;
volatile u32
    usb_bulk_rx_errors; /* transfers dropped for a halted, buffer-error or transaction-error status */
volatile u32
    usb_bulk_rx_inval_lines; /* data cache lines invalidated by usb_bulk_take (0 with UB_CACHED_RX 0); the emulator has no data cache model */

/* UB_CACHED_RX 1: usb_bulk_take hands out the CACHED address of a receive buffer, after it invalidated the buffer's data cache lines, so the module's reads
 * fill whole 16-byte lines in bursts (the uncached alias costs one bus access per longword). 0: the uncached alias, as before (the A/B switch).
 * DEFAULT 0 until the emulator aliases DDR: its board keeps the cached window 0x40000000 and the uncached alias +0x08000000 in separate pages
 * (packages/emulator/machine/src/board.rs page_index), so the emulator's DMA data (written at the alias) reads as zeros at the cached address. Hardware: -DUB_CACHED_RX=1.
 * Rules, all in this file:
 *  1. The CPU never writes a receive buffer through its cached address. Only the USB DMA writes these buffers (enable writes only the descriptor, through the
 *     alias; the receive buffers are never written by the CPU). A dirty line could be written back over DMA data later. The cached address exists only as the
 *     result of rx_cached(), which only usb_bulk_take returns, and the module reads it (its parameter is documented read-only).
 *  2. usb_bulk_take invalidates the lines of the received bytes after the DMA completed (the completion callback marked the buffer full) and before the
 *     first CPU read. `cpushl dc,(Ax)` pushes a line if modified and invalidates it (CACR[DDPI] = 0, CFPRM CPUSHL p.8-2, MCF5441x RM 6.4.8 Table 6-10). The firmware
 *     writes CACR = 0xa50ce100 once (finding 07-emulator): SPA (bit 14) = 1 so Ax is a physical address and only the line that holds it is touched, IVO = 0,
 *     DDPI = 0. The lines are clean (rule 1), so the push writes nothing. The caches do not snoop DMA (RM 6.1).
 *  3. The cached address is returned only after the invalidate; usb_bulk_release takes either address. */
#ifndef UB_CACHED_RX
#define UB_CACHED_RX 0
#endif

static inline u32 sr_mask(void)
{
    u32 old;
    __asm__ volatile("move.w %%sr,%0\n\tmove.w #0x2700,%%sr" : "=d"(old) : : "memory", "cc");
    return old;
}

static inline void sr_restore(u32 old)
{
    __asm__ volatile("move.w %0,%%sr" : : "d"(old) : "memory", "cc");
}

static u8 *alias_of(u8 *p)
{
    return p + usb_alias_delta;
}

static u8 *rx_buf(u32 i)
{
    return alias_of(rx_area + i * RX_BYTES);
}

static u8 *tx_buf(u32 i)
{
    return alias_of(tx_area + i * TX_BYTES);
}

static u8 *desc_buf(void)
{
    return alias_of(desc_mem);
}

/* the index of the receive buffer that `p` points into (the uncached alias, or the cached address that usb_bulk_take may have returned), or -1 */
static i32 rx_index(const u8 *p)
{
    u32 off;
    if (!live)
        return -1;
    if ((u32)p < (u32)rx_buf(0))
        p += usb_alias_delta; /* the cached address of the same memory */
    off = (u32)(p - rx_buf(0));
    return off < rx_slots * RX_BYTES ? (i32)(off / RX_BYTES) : -1;
}

static i32 tx_index(const u8 *p)
{
    u32 off;
    if (!live)
        return -1;
    off = (u32)(p - tx_buf(0));
    return off < TX_SLOTS * TX_BYTES ? (i32)(off / TX_BYTES) : -1;
}

static volatile u32 *qh(u32 off)
{
    return (volatile u32 *)(QH_BASE + off);
}

static void wake(void)
{
    i32 j = io_job;
    if (j > 0)
        dkrt_io_wake(j);
}

/* The free callback of every dTD of ours (+0x38): note its status byte (the completion callback that follows reads it: the engine passes only the byte
 * count), then the firmware's own, which returns the dTD to the pool. The completion loop of the engine (vector_134_handler) reads the dTD's next pointer
 * after this callback and does not touch the dTD again after the completion callback, so a dTD handed out again by the completion callback is safe. */
void usb_bulk_td_free(u8 *dtd)
{
    td_status = dtd[7];
    ((void (*)(u8 *))stock_free)(dtd);
}

/* A dTD for `bytes` of `data` (pages from a 4 KiB aligned address) with our callbacks; queued on queue `index`. 0 when queued. The firmware's queue
 * function sets the interrupt-on-completion bit because a callback is set. Caller: interrupts masked.
 * Page pointers 1-4 always name the four pages after page 0, as RM 24-71 asks. The controller can use the pointer of the page where a transfer ends
 * (a zero-length packet after a full page); a zero pointer there made the DMA fault and wedged the controller on hardware (2026-11-11). The pages past
 * a short transfer are never read or written (the dTD's total bytes bound the data). The block is allocated with two pages after the area, so the pointers
 * of the last buffer (the fifth names the page after it) stay inside the block we own. */
static i32 queue_transfer(u32 index, u8 *data, u32 bytes, void *done, void *cancel)
{
    u32 *w = (u32 *)usb_fw_td_alloc();
    u32 k;
    if (w == 0)
        return -1;
    if (w[14] != (u32)usb_bulk_td_free)
        stock_free = w[14];
    w[0] = TD_END;
    w[1] = (bytes << 16) | TD_ACTIVE;
    w[2] = (u32)data;
    for (k = 1; k < 5; k++)
        w[2 + k] = (u32)data + k * PAGE;
    w[8] = bytes;                  /* +0x20: the length the firmware subtracts the remainder from */
    w[9] = (u32)done;              /* +0x24 */
    w[10] = (u32)cancel;           /* +0x28 */
    w[14] = (u32)usb_bulk_td_free; /* +0x38 */
    if (usb_fw_queue_td(index, (u8 *)w) < 0) {
        /* the engine refused (the endpoint is stalled): give the dTD back through the firmware's own free callback */
        ((void (*)(u8 *))stock_free)((u8 *)w);
        return -1;
    }
    return 0;
}

void usb_bulk_rx_done(u32 bytes, u8 *buf);
void usb_bulk_tx_done(u32 bytes, u8 *buf);
void usb_bulk_cancel(u32 bytes, u8 *buf);

/* EP2 IN is the audio clock feedback in USB AUDIO/MIDI: a dTD of ours there could fault the controller, so nothing is queued outside USB MIDI mode */
static int usb_midi_mode(void)
{
    return *(volatile u32 *)usb_active_port == PORT_ENTRY;
}

/* `p5_attached`: core/usb-bulk-audio extended entry 5 (usb_bulk_port5_attach); set once, from the boot hook. In USB AUDIO/MIDI with it the interface is OUT only (bit
 * DK_USB_BULK_OUT_ONLY of the state word): the receive ring runs on EP2 OUT, nothing is sent. `conn_rx`: the host connection is the port 5 one (EP2 OUT only), read when
 * the buffers are freed. */
static volatile u32 p5_attached, conn_rx;

static int usb_p5_mode(void)
{
    return p5_attached && *(volatile u32 *)usb_active_port == PORT_AUDIO;
}

/* the active port can carry the receive ring: USB MIDI (the pair), or USB AUDIO/MIDI with the OUT-only interface */
static int bulk_port(void)
{
    return usb_midi_mode() || usb_p5_mode();
}

/* the slots in use now: the whole ring, or RX_OLD of it with the switch off */
static u32 rx_use(void)
{
    return !usb_bulk_ring_off || rx_slots < RX_OLD ? rx_slots : RX_OLD;
}

static void prime_one(u32 i)
{
    if (stopping || !live || !connected || !bulk_port())
        return;
    if (queue_transfer(IDX_OUT, rx_buf(i), RX_BYTES, (void *)usb_bulk_rx_done,
                       (void *)usb_bulk_cancel) == 0) {
        rx_state[i] = B_PRIMED;
        UB_LOG("ub.rx.prime", i);
    }
}

/* Queue free slots round the ring until RX_INFLIGHT are on the controller (all of them with the ring off: at most RX_OLD). The next completion queues more, so this is the
 * interrupt's re-prime too. A slot the engine refused (the dTD pool was empty) stops the scan and leaves `need_prime` set for the tick and the module's next call. */
static void prime_all(void)
{
    u32 n, i, use, cap, primed = 0, left = 0;
    if (stopping || !live || !connected)
        return;
    if (!bulk_port()) {
        need_prime = 1; /* resumes when the mode is back and the firmware reconnects */
        return;
    }
    use = rx_use();
    cap = use <= RX_OLD ? use : RX_INFLIGHT;
    for (i = 0; i < rx_slots; i++)
        if (rx_state[i] == B_PRIMED)
            primed++;
    for (n = 0; n < use && primed < cap; n++) {
        i = (prime_cur + n) % use;
        if (rx_state[i] != B_FREE)
            continue;
        prime_one(i);
        if (rx_state[i] != B_PRIMED) {
            left = 1;
            break;
        }
        primed++;
        prime_cur = (i + 1u) % use;
    }
    need_prime = left;
}

/* ---- the dTD callbacks ---- */

/* EP2 OUT transfer complete (interrupt): `bytes` arrived in `buf`. A zero-length packet (the end of a message that was a multiple of 512 bytes
 * after a full buffer) holds no data: the buffer is queued again. A status with an error bit drops the transfer. */
void usb_bulk_rx_done(u32 bytes, u8 *buf)
{
    u32 sr = sr_mask();
    i32 i = rx_index(buf);
    u32 bad = td_status & (TD_HALTED | TD_BUFERR | TD_XACTERR);
    td_status = 0;
    to_active = 1; /* host activity for the take-over watchdog */
    UB_LOG("ub.rx.done", bytes | ((u32)i << 24));
    if (i >= 0 && rx_state[i] == B_PRIMED) {
        if (bad != 0u) {
            usb_bulk_rx_errors++;
            UB_LOG("ub.rx.err", bad);
            rx_state[i] = B_FREE;
            prime_all();
        } else if (bytes == 0) {
            rx_state[i] = B_FREE;
            prime_all();
        } else {
            rx_len[i] = bytes;
            rx_state[i] = B_FULL;
            order[(order_head + order_count) % RX_MAX] = (u8)i;
            order_count++;
            prime_all();
            wake();
        }
    }
    sr_restore(sr);
}

/* EP2 IN transfer complete (interrupt): the slot is free; a step waiting for one is woken. */
void usb_bulk_tx_done(u32 bytes, u8 *buf)
{
    u32 sr = sr_mask();
    i32 i = tx_index(buf);
    (void)bytes;
    to_active = 1;
    UB_LOG("ub.tx.done", bytes);
    td_status = 0;
    if (i >= 0) {
        tx_state[i] = T_FREE;
        wake();
    }
    sr_restore(sr);
}

/* A queued transfer was cancelled (ep2_stop): the buffer is free again, nothing arrived and nothing is queued in its place. */
void usb_bulk_cancel(u32 bytes, u8 *buf)
{
    u32 sr = sr_mask();
    i32 i = rx_index(buf);
    (void)bytes;
    UB_LOG("ub.cancel", bytes);
    td_status = 0;
    if (i >= 0) {
        if (rx_state[i] == B_PRIMED)
            rx_state[i] = B_FREE;
    } else {
        i = tx_index(buf);
        if (i >= 0)
            tx_state[i] = T_FREE;
    }
    sr_restore(sr);
}

/* ---- the firmware's callbacks of port entry 2 ---- */

/* The EP2 queue heads and the engine's lists of queued dTDs (+0x30 head, +0x34 tail), empty. */
static void ep2_lists_clear(u32 rx_only)
{
    volatile u32 *q = qh(QH_EP2_OUT);
    q[1] = 0;
    q[2] = TD_END;
    q[3] = 0;
    q[0x30 / 4] = 0;
    q[0x34 / 4] = 0;
    if (rx_only)
        return;
    q = qh(QH_EP2_IN);
    q[1] = 0;
    q[2] = TD_END;
    q[3] = 0;
    q[0x30 / 4] = 0;
    q[0x34 / 4] = 0;
}

/* the ENDPTFLUSH bits of EP2: both directions, or EP2 OUT only (`rx_only`: USB AUDIO/MIDI) */
static u32 ep2_bits(u32 rx_only)
{
    return rx_only ? FLUSH_EP2_RX : FLUSH_EP2;
}

/* Flush EP2 with a bounded poll. 1 when the controller has finished the flush. */
static u32 ep2_flush(u32 rx_only)
{
    u32 spins, m = ep2_bits(rx_only);
    USB_REG(ENDPTFLUSH) = m;
    for (spins = 0; spins < FLUSH_SPINS && (reg_rd(ENDPTFLUSH) & m) != 0u; spins++)
        ;
    return (reg_rd(ENDPTFLUSH) & m) == 0u;
}

/* No transfer of EP2 is flushing, primed or active in the controller (both directions, or EP2 OUT only). */
static u32 ep2_idle(u32 rx_only)
{
    return ((reg_rd(ENDPTFLUSH) | reg_rd(ENDPTSTAT) | reg_rd(ENDPTPRIME)) & ep2_bits(rx_only)) ==
           0u;
}

/* EP2 off: flush with a bounded poll, cancel both queues (their callbacks run with `stopping` set: they queue nothing), then empty the lists. Every buffer
 * that was queued is free again; full buffers that nobody took belong to the connection that ended and are dropped; a taken buffer stays with its
 * owner. The reset count changes and the job is woken. Interrupts masked by the caller. `rx_only` (USB AUDIO/MIDI): only the receive half of ENDPTCTRL2 and EP2 OUT are
 * touched, EP2 IN is the firmware's audio feedback. */
static void ep2_stop(u32 rx_only)
{
    u32 i;
    UB_LOG("ub.ep2stop", resets);
    stopping = 1;
    if (!qh_ready)
        ep2_lists_clear(
            rx_only); /* the engine's init cleared the queue heads, but another USB mode (Overbridge also uses EP2) may have left words */
    USB_REG(ENDPTCTRL2) = rx_only ? reg_rd(ENDPTCTRL2) & ~CTRL_RX_MASK : 0u;
    ep2_flush(rx_only);
    usb_fw_cancel_ep(IDX_OUT);
    if (!rx_only)
        usb_fw_cancel_ep(IDX_IN);
    ep2_lists_clear(rx_only);
    USB_REG(ENDPTCOMPLETE) = ep2_bits(
        rx_only); /* write 1 to clear: a completion of the old connection must not be seen by the engine's loop */
    for (i = 0; i < rx_slots; i++)
        if (rx_state[i] != B_TAKEN)
            rx_state[i] = B_FREE;
    order_head = order_count = 0;
    for (i = 0; i < TX_SLOTS; i++)
        if (tx_state[i] !=
            T_RESERVED) /* a send is copying into this slot (usb_bulk_send, interrupts on): it frees the slot itself when it sees the reset */
            tx_state[i] = T_FREE;
    need_prime = 0;
    if (!rx_only)
        qh_ready = 1; /* only the full clear covers the EP2 IN lists */
    if (connected) {
        resets++;
        connected = 0;
        wake();
    }
    stopping = 0;
}

/* Take every queued transfer of EP2 back without disabling the endpoint (the host gets NAK; the descriptor keeps the interface): flush, cancel, empty lists. Receive
 * buffers held by the module and send slots being filled stay as they are. Interrupts masked by the caller; EP2 must be ours (`connected`). `rx_only`: as ep2_stop. */
static void ep2_quiesce(u32 rx_only)
{
    u32 i;
    stopping = 1;
    ep2_flush(rx_only);
    usb_fw_cancel_ep(IDX_OUT);
    if (!rx_only)
        usb_fw_cancel_ep(IDX_IN);
    ep2_lists_clear(rx_only);
    USB_REG(ENDPTCOMPLETE) = ep2_bits(rx_only);
    for (i = 0; i < rx_slots; i++)
        if (rx_state[i] != B_TAKEN)
            rx_state[i] = B_FREE;
    order_head = order_count = 0;
    for (i = 0; i < TX_SLOTS; i++)
        if (tx_state[i] != T_RESERVED)
            tx_state[i] = T_FREE;
    need_prime = 0;
    stopping = 0;
}

/* the host's bus reset or resume: the stock work of the port (EP1 for MIDI), then EP2 */
void usb_bulk_connect(void)
{
    u32 sr = sr_mask();
    sw_cb(&sw_con_cnt, &sw_con_at, &sw_con_ps);
    ep2_stop(0);
    stock_connect(1); /* the wrapper reads and sets its argument slot */
    qh(QH_EP2_OUT)[0] = QH_CAPS_OUT;
    qh(QH_EP2_IN)[0] = QH_CAPS_IN;
    if ((reg_rd(ENDPTSTAT) & FLUSH_EP2) != 0u)
        ep2_flush(0); /* the controller still holds a transfer of the old connection */
    USB_REG(ENDPTCTRL2) = CTRL_BULK;
    connected = 1;
    conn_rx = 0;
    prime_all();
    sr_restore(sr);
}

/* USB AUDIO/MIDI, called by the port 5 wrappers of core/usb-bulk-audio. No runtime calls (the boot hook and the USB interrupt run before the runtime exists or without it);
 * ISR-safe; never stops the controller. `attach`: entry 5 now carries the OUT-only interface. */
void usb_bulk_port5_attach(void)
{
    p5_attached = 1;
}

/* After the stock port 5 connect (a bus reset or SET_CONFIGURATION: the dTDs of the old connection are gone without callbacks, so the ring is rebuilt): EP2 OUT empty, the
 * queue head caps set, the receive half of ENDPTCTRL2 on (the transmit half stays the firmware's feedback endpoint), the ring queued when the buffers exist. */
void usb_bulk_port5_connect(void)
{
    u32 sr = sr_mask();
    ep2_stop(1);
    qh(QH_EP2_OUT)[0] = QH_CAPS_OUT;
    if ((reg_rd(ENDPTSTAT) & FLUSH_EP2_RX) != 0u)
        ep2_flush(1); /* the controller still holds a transfer of the old connection */
    USB_REG(ENDPTCTRL2) = reg_rd(ENDPTCTRL2) | CTRL_RX_BULK;
    connected = 1;
    conn_rx = 1;
    prime_all();
    sr_restore(sr);
}

/* Before the stock port 5 disconnect: the receive half of ENDPTCTRL2 off, EP2 OUT flushed and cancelled, the ring idle. */
void usb_bulk_port5_disconnect(void)
{
    u32 sr = sr_mask();
    ep2_stop(1);
    sr_restore(sr);
}

static u32 sw_guard_open(void); /* take-over section */

/* the host is gone or the mode switches: EP2 first, then the stock callback */
void usb_bulk_disconnect(void)
{
    u32 sr = sr_mask();
    sw_cb(&sw_dis_cnt, &sw_dis_at, &sw_dis_ps);
    if (taken && !to_quiet && sw_guard_open())
        to_gone =
            1; /* unplug, suspend or a mode change while taken over: the tick decides (this runs in the USB interrupt) */
    ep2_stop(0);
    sr_restore(sr);
    usb_fw_disconnect();
}

/* ---- the buffers: allocated when the interface can go live, freed when the take-over ends ---- */

/* The block: the receive slots (ring_tiers), then TX_SLOTS send slots, 4 KiB aligned, and two pages after them (the dTD page pointers of the last buffer). The firmware's
 * buddy allocator (heap at 0x44f39c40 on the DT2: not 4 KiB aligned itself) gives a block that is 16 byte aligned: the area is rounded up inside it. The allocator
 * rounds the size up to a power of two (256 KiB), the cost while live. The firmware's operator new and free take the heap mutex themselves (the anchors
 * cf.mem.site.heap.lock_lit and unlock_lit lie inside cf.mem.fn.new; finding 01), so they may be called from the I/O, MIDI and UI tasks, never from an interrupt. */
/* The size asked from the allocator for `n` receive slots: the area, the two pages after it, and a page of slack for the 4 KiB rounding of the start (the allocator's
 * block is only 16-byte aligned). 60 slots ask 1,028,096 bytes (a 1 MiB block), 28 ask 503,808 (a 512 KiB block), 8 ask 172 KiB (a 256 KiB block). */
#define BLOCK_AREA(n) ((n) * RX_BYTES + TX_SLOTS * TX_BYTES)
#define BLOCK_BYTES(n) (BLOCK_AREA(n) + 3u * PAGE)
/* the tiers tried in order, the largest first; with the ring off only the last */
static const u8 ring_tiers[3] = {60, 28, 8};

/* Allocate the buffers (task context, never under the mask or in an interrupt). 0 when live (also when it already was). E_STATE: another caller is allocating.
 * E_NOMEM: no memory, or the block is outside the cached DDR window the alias arithmetic needs (ST_NOMEM shows it; nothing is changed). The area is pushed
 * out of the data cache (the allocator's earlier owner may have left dirty lines) and used through the alias only. When EP2 is up already (the host connected
 * before the buffers existed, or after they were freed), the buffers are queued at once, under the mask. */
static i32 buffers_alloc(void)
{
    u32 sr, i, n, t;
    u8 *raw, *a;
    sr = sr_mask();
    if (live) {
        sr_restore(sr);
        return 0;
    }
    if (alloc_busy) {
        sr_restore(sr);
        return E_STATE;
    }
    alloc_busy = 1;
    sr_restore(sr);
    /* the largest tier the heap gives (the ring switch is read once, here); a failure of every tier is E_NOMEM, never a half-enabled ring */
    n = 0;
    raw = 0;
    a = 0;
    for (t = usb_bulk_ring_off ? 2u : 0u; t < 3u; t++) {
        u32 want = ring_tiers[t];
        u8 *r = usb_bulk_tier_cap != 0u && want > usb_bulk_tier_cap
                    ? 0
                    : usb_fw_mem_new(BLOCK_BYTES(
                          want)); /* the cap: the emulator test's way to make a tier fail */
        u8 *al = (u8 *)(((u32)r + PAGE - 1u) & ~(PAGE - 1u));
        if (r != 0 && (u32)al >= usb_ddr_base &&
            (u32)al + BLOCK_AREA(want) + PAGE <= usb_ddr_base + usb_ddr_bytes) {
            raw = r;
            a = al;
            n = want;
            break;
        }
        if (r != 0)
            usb_fw_mem_free(r);
    }
    if (n == 0u) {
        nomem = 1;
        alloc_busy = 0;
        return E_NOMEM;
    }
    for (i = 0; i < BLOCK_AREA(n); i += 16u)
        __asm__ volatile("cpushl %%bc,(%0)" : : "a"((u32)a + i) : "memory");
    usb_bulk_last_area = (u32)a;
    usb_bulk_last_bytes = BLOCK_AREA(n);
    usb_bulk_last_slots = n;
    sr = sr_mask();
    blk_raw = raw;
    rx_area = a;
    tx_area = a + n * RX_BYTES;
    rx_slots = n;
    prime_cur = 0;
    for (i = 0; i < n; i++)
        rx_state[i] = B_FREE;
    for (i = 0; i < TX_SLOTS; i++)
        tx_state[i] = T_FREE;
    order_head = order_count = 0;
    need_prime = 0;
    nomem = 0;
    live = 1;
    alloc_busy = 0;
    if (connected && bulk_port())
        prime_all();
    sr_restore(sr);
    return 0;
}

/* Free the buffers when nothing can touch them (all under the mask, so neither the interrupt nor a task changes anything meanwhile):
 *  - no receive buffer is held by the module (B_TAKEN) and no send slot is being filled (T_RESERVED): those are the CPU's, nothing is freed yet;
 *  - EP2 is ours (`connected`: the host configured it and no disconnect ran since): its transfers are taken back without disabling the endpoint (ep2_quiesce:
 *    flush, cancel, empty lists), and the controller must then report EP2 idle: ENDPTFLUSH clear and neither primed nor active in ENDPTPRIME and ENDPTSTAT,
 *    both directions. If not, nothing is freed and the tick asks again (`need_prime` re-queues the buffers meanwhile);
 *  - EP2 is not ours (no connect since the last disconnect, which ran ep2_stop: ENDPTCTRL2 off, queues cancelled): the endpoint is not touched, because it may
 *    belong to the firmware (the audio clock feedback of USB AUDIO/MIDI); it is enough that none of our transfers is queued.
 * Then `live` is 0 (every queue path tests it) and the block is freed. Returns 1 when the buffers are gone. A free that waits for the allocation in progress
 * keeps `free_req`. */
static u32 buffers_free(void)
{
    u32 sr, i;
    u8 *raw;
    sr = sr_mask();
    if (alloc_busy) {
        usb_bulk_free_why = 16;
        sr_restore(sr);
        return 0;
    }
    if (!live) {
        free_req = 0;
        sr_restore(sr);
        return 1;
    }
    for (i = 0; i < rx_slots; i++)
        if (rx_state[i] == B_TAKEN) {
            usb_bulk_free_why = 1;
            sr_restore(sr);
            return 0;
        }
    for (i = 0; i < TX_SLOTS; i++)
        if (tx_state[i] == T_RESERVED) {
            usb_bulk_free_why = 2;
            sr_restore(sr);
            return 0;
        }
    if (connected) {
        u32 m = ep2_bits(conn_rx);
        ep2_quiesce(conn_rx);
        if (!ep2_idle(conn_rx)) {
            usb_bulk_free_why = 4 | (((reg_rd(ENDPTFLUSH) & m) != 0u ? 1u : 0u) |
                                     ((reg_rd(ENDPTSTAT) & m) != 0u ? 2u : 0u) |
                                     ((reg_rd(ENDPTPRIME) & m) != 0u ? 4u : 0u))
                                        << 8;
            need_prime = 1;
            sr_restore(sr);
            return 0;
        }
    } else {
        for (i = 0; i < rx_slots; i++)
            if (rx_state[i] == B_PRIMED) {
                usb_bulk_free_why = 8;
                sr_restore(sr);
                return 0;
            }
        for (i = 0; i < TX_SLOTS; i++)
            if (tx_state[i] == T_QUEUED) {
                usb_bulk_free_why = 8;
                sr_restore(sr);
                return 0;
            }
    }
    usb_bulk_free_why = 0;
    live = 0;
    raw = blk_raw;
    blk_raw = 0;
    rx_area = tx_area = 0;
    for (i = 0; i < rx_slots; i++)
        rx_state[i] = B_FREE;
    rx_slots = 0;
    for (i = 0; i < TX_SLOTS; i++)
        tx_state[i] = T_FREE;
    order_head = order_count = 0;
    need_prime = 0;
    free_req = 0;
    sr_restore(sr);
    usb_fw_mem_free(raw);
    return 1;
}

/* ---- take-over ---- */

/* Guard after every mode switch. The firmware restarts the device after a delay of 30 ticks of its USB tick (FUN_4000713e counts the word of
 * cf.usb.value.restart_delay down from 30 and restarts at 0). Measured in the emulator on both images: about 1 s (33 ms a tick). That word is the exact answer to "is a restart
 * pending" and does not depend on any clock of ours, so the guard is open only when it is 0 (fw_restart_pending). TO_GUARD_MS is a floor on our own clock from the
 * stamp: it covers the moment between the stamp and the firmware setting the word, and a word that cannot be read. */
#define TO_GUARD_MS                                                                                \
    3500u /* the hold below (2 s) and the host's re-enumeration (1.3 to 1.5 s on macOS, finding 123) before the next switch may run */
#define TO_DELAY_MS                                                                                \
    50u /* a requested switch runs this long after the tick first saw it (the reply is out by then) */
#define TO_IDLE_MS 30000u  /* no host activity for this long: give back */
#define TO_SUSPEND_MS 500u /* the bus stayed suspended this long: give back */
#define TO_QUIET_MS                                                                                \
    2000u /* the suspend check starts this long after the last activity (a switch counts as activity) */

/* the tick clock: the runtime's block clock (1500 a second), or the passes of the UI loop (about 120 a second) without one, as core/ui does */
static u32 to_peek(u32 *per_s)
{
    u32 now = dkrt_blocks();
    if (now != 0u) {
        *per_s = 1500u;
        return now;
    }
    *per_s = 120u;
    return to_passes;
}

/* the tick's clock: one more pass of the UI loop when there is no block clock */
static u32 to_clock(u32 *per_s)
{
    to_passes++;
    return to_peek(per_s);
}

static u32 to_elapsed_ms(u32 since, u32 now, u32 per_s)
{
    u32 d = now - since;
    return d / per_s * 1000u + d % per_s * 1000u / per_s;
}

/* The firmware's restart delay (anchor cf.usb.value.restart_delay): 30 (60 with our hold) down to 0 by the USB tick, non-zero while a stop is waiting to restart.
 * A larger value is not the counter and counts as no restart pending. */
#define FW_STOP_TICKS 30u /* what the firmware's stop sets */
/* The hold: after our stop the adapter raises the counter to this, so the device stays off the bus for about 2 s (a tick is 33.3 ms) instead of 1 s. Macos noticed the
 * detach within 0.2 to 0.3 s when it noticed it at all (finding 123), so 2 s is about six times that and covers the hub's 100 ms debounce and the OS teardown; more only makes
 * every switch slower. */
#define FW_HOLD_TICKS 60u
#define FW_RESTART_TICKS FW_HOLD_TICKS /* the largest value the counter can have */
#define HOLD_RESTORE_TICKS                                                                         \
    3u /* the PHY is put back this many ticks (100 ms) before the firmware restarts */
#define TO_BUSY_MS 300u /* a switch waits at most this long for a reply on EP0 or EP1 IN */
#define TO_FORCE_MS                                                                                \
    4000u /* a restart that has not happened this long after a switch is forced (hold 2 s plus margin) */

static u32 fw_restart_pending(void)
{
    u32 d = *(volatile u32 *)usb_restart_delay;
    return d != 0u && d <= FW_RESTART_TICKS;
}

/* Is a new switch allowed? Interrupts masked by the caller. When the clock source changed since the stamp the guard starts again. */
static u32 sw_guard_open(void)
{
    u32 per, now;
    if (to_pend_port != 0u || fw_restart_pending())
        return 0;
    if (!to_sw_set)
        return 1;
    now = to_peek(&per);
    if (per != to_sw_per) {
        to_sw_per = per;
        to_sw_at = now;
        return 0;
    }
    return to_elapsed_ms(to_sw_at, now, per) >= TO_GUARD_MS;
}

/* ---- the switch timeline ----
 * USB_SW events (sdk.toml kind 44, level info; layout in README.md) for one take-over or release: from the accepted request until the host has configured the device
 * (DONE) or SW_WINDOW_MS have passed (TIMEOUT). UI task only. The tick samples the controller once a pass and logs a milestone when it first shows; the callbacks that run in
 * the interrupt are logged from their stamps. At most SW_MAX_EVENTS events a window, STATE stops at SW_STATE_MAX (DONE, TIMEOUT, GIVEUP and DROPPED always log). Switch
 * number: the switch this request makes. */
enum {
    SW_REQ = 1,
    SW_WAIT,
    SW_FW_SWITCH,
    SW_DISCONNECT_CB,
    SW_HOLD_BEGIN,
    SW_PHY_PARK,
    SW_PHY_RESTORE,
    SW_RS_UP,
    SW_CONNECT_CB,
    SW_ADDRESSED,
    SW_CONFIGURED,
    SW_STATE,
    SW_ENUM_LATE,
    SW_RECOVERY,
    SW_GIVEUP,
    SW_DONE,
    SW_TIMEOUT,
    SW_DROPPED,
    SW_PHY_BUSY,
    SW_PHY_ID,
    SW_PHY_WAKE
};
#define SW_KIND 44u
#define SW_FLAGS (2u << 1)       /* level 2 (info) in bits 1-3 */
#define SW_FLAGS_STATE (3u << 1) /* STATE: level 3 (debug) */
#define SW_FLAGS_WARN (1u << 1)  /* ENUM_LATE, TIMEOUT: level 1 (warn) */
#define SW_FLAGS_ERROR 1u        /* GIVEUP: level 0 (error) and the error bit */
#define SW_MAX_EVENTS 32u
#define SW_STATE_MAX 24u
#define SW_WINDOW_MS 10000u
#define SW_LATE_MS 3000u
#define SW_NOCONF_MS 2000u /* DONE without CONFIGURED, this long after ADDRESSED */
/* `last`: the sampled words of the previous pass; `fc`: the last ULPI function control value seen (0xff unknown); `ov_ms`, `ov_ps`: the stamp of a callback event (~0 / 0: none).
 * `reset_seen`: after the switch a bus reset showed (connect callback, URI, or address 0); `en_cleared`: the EP1 enables were seen 0; `addr_at`: the time of ADDRESSED.
 * `hold_ov`: the events of the switch carry the time and words stamped right after usb_fw_mode_switch (`hold_ms`, `hold_w`).
 * `win`: the window ends SW_WINDOW_MS after this time (ms; a recovery moves it). `rec_n`: ENUM_LATE controller resets started for this switch; `gave`: the watchdog gave up. */
static struct {
    u32 start, per, no, target, dir, n, seen, dis, con, last_fr, rs_at, stopped, waited, late;
    u32 reset_seen, en_cleared, addr_at, hold_ov, hold_ms, win, rec_n, gave, sof, fw;
    u32 last[3], hold_w[3];
    u32 last_gpt, clk;
} sw;
/* `sw.sof`: FRINDEX changed since RS_UP. */
static u32 sw_fc = 0xffu, sw_ov_ms = ~0u, sw_ov_ps;
/* `sw_x_set`: the next event takes words 2 (bit 0) and 3 (bit 1) from `sw_x_a2` and `sw_x_a3` (PHY_BUSY, late PHY_PARK) */
static u32 sw_x_set, sw_x_a2, sw_x_a3;

static u32 sw_ms(void)
{
    u32 per, now = to_peek(&per);
    return per == sw.per ? to_elapsed_ms(sw.start, now, per) : 0u;
}

/* RXE (bit 7) and TXE (bit 23) of an ENDPTCTRL word as bits 0 and 1 */
static u32 sw_ep(u32 c)
{
    return ((c >> 7) & 1u) | ((c >> 22) & 2u);
}

/* words 2 to 4 of an event: USBSTS and RS and the PHY value, PORTSC, and the OTG word */
static void sw_words(u32 *a)
{
    u32 otg = reg_rd(OTGSC), fr = reg_rd(FRINDEX);
    u32 en = sw_ep(reg_rd(ENDPTCTRL1)) | (sw_ep(reg_rd(ENDPTCTRL2)) << 2) |
             (sw_ep(reg_rd(ENDPTCTRL3)) << 4);
    u32 audio = ((reg_rd(ENDPTSTAT) | reg_rd(ENDPTPRIME)) >> 19) & 1u;
    u32 gpt = reg_rd(GPTIMER0CTL) & 0xffffffu;
    a[2] = (reg_rd(USBSTS) & 0xffffu) | ((reg_rd(USBCMD) & USBCMD_RS) << 16) | (sw_fc << 24);
    a[3] = reg_rd(PORTSC);
    a[4] = ((otg >> 11) & 1u) | ((u32)(fr != sw.last_fr) << 1) | (audio << 2) |
           ((u32)(gpt != sw.last_gpt) << 3) | (((reg_rd(DEVICEADDR) >> 25) & 0x7fu) << 8) |
           (en << 16) | ((sw_con_cnt & 0xffu) << 24);
}

static void sw_ev(u32 phase, u32 detail)
{
    u32 a[5];
    if (!sw_open)
        return;
    sw.seen |= 1u << phase;
    if (phase == SW_STATE ? sw.n >= SW_STATE_MAX
                          : sw.n >= SW_MAX_EVENTS && phase != SW_DONE && phase != SW_TIMEOUT &&
                                phase != SW_GIVEUP && phase != SW_DROPPED && phase != SW_RECOVERY)
        return;
    sw.n++;
    sw_words(a);
    if (sw.hold_ov) {
        a[2] = sw.hold_w[0];
        a[3] = sw.hold_w[1];
        a[4] = sw.hold_w[2];
    }
    if ((sw_x_set & 1u) != 0u)
        a[2] = sw_x_a2;
    if ((sw_x_set & 2u) != 0u)
        a[3] = sw_x_a3;
    a[0] = phase | ((sw.target & 0xfu) << 8) | (sw.dir << 12) | ((detail & 7u) << 13) |
           ((sw.no & 0xffffu) << 16);
    a[1] = sw_ov_ms != ~0u ? sw_ov_ms : sw.hold_ov ? sw.hold_ms : sw_ms();
    if (sw_ov_ps != 0u)
        a[3] = sw_ov_ps;
    sw_ov_ms = ~0u;
    sw_ov_ps = 0;
    dkrt_log_ev(SW_KIND,
                phase == SW_STATE ? SW_FLAGS_STATE
                : phase == SW_ENUM_LATE || phase == SW_TIMEOUT || phase == SW_PHY_BUSY
                    ? SW_FLAGS_WARN
                : phase == SW_GIVEUP ? SW_FLAGS_ERROR
                                     : SW_FLAGS,
                a);
}

/* An accepted request (usb_bulk_takeover, usb_bulk_giveback, the automatic give-back) opens the window; REQ is logged here, from the tick. */
static void sw_begin(u32 dir, u32 target, u32 no, u32 fw)
{
    u32 per, a[5];
    sw_open = 0;
    sw.start = to_peek(&per);
    sw.per = per;
    sw.dir = dir;
    sw.target = target;
    sw.no = no;
    sw.n = sw.seen = sw.stopped = sw.waited = sw.late = 0;
    sw.reset_seen = sw.en_cleared = sw.addr_at = sw.hold_ov = 0;
    sw.win = sw.rec_n = sw.gave = sw.sof = 0;
    sw.fw = fw;
    sw.dis = sw_dis_cnt;
    sw.con = sw_con_cnt;
    sw_open = 1;
    sw_words(a);
    sw.last[0] = a[2];
    sw.last[1] = a[3];
    sw.last[2] = a[4] & ~0xau;
    sw.last_fr = reg_rd(FRINDEX);
    sw.last_gpt = reg_rd(GPTIMER0CTL) & 0xffffffu;
    sw.clk = 0;
    sw_ev(SW_REQ, fw ? 4u : 0u);
    if (fw) /* the firmware's switch was made before the window opened: no FW_SWITCH event, the sampler goes on as after it */
        sw.seen |= 1u << SW_FW_SWITCH;
}

static void sw_pend_poll(void)
{
    u32 sr, d;
    if (!sw_pend)
        return;
    sr = sr_mask();
    d = sw_pend;
    sw_pend = 0;
    sr_restore(sr);
    sw_begin(d - 1u, d == 1u ? PORT_ENTRY : to_orig,
             usb_bulk_switches + (to_pend_port == 0u ? 1u : 0u), 0);
}

/* A mode switch the adapter did not ask for (the stock USB CONFIG menu calls the firmware's switch itself): when the active port changes with no switch of ours in
 * flight (no claimed switch, no request, no open window), log it as a REQ with detail 4. The window is never recovered (wd_tick skips `sw.fw`). Once a pass,
 * after takeover_tick (our own switch is made there and opens its window before this runs). */
static void sw_fw_poll(void)
{
    static u32 last_port, last_set;
    u32 port = *(volatile u32 *)usb_active_port;
    if (!last_set) {
        last_set = 1;
        last_port = port;
        return;
    }
    if (port == last_port)
        return;
    last_port = port;
    if (sw_open || sw_pend || to_pend_port != 0u || to_req != 0u)
        return;
    sw_begin(port == PORT_ENTRY ? 0u : 1u, port, usb_bulk_switches, 1);
}

/* the time of a callback stamp (block clock), or `ms` when it has none */
static u32 sw_cb_ms(u32 at, u32 ms)
{
    return at != 0u && sw.per == 1500u ? to_elapsed_ms(sw.start, at, 1500u) : ms;
}

#define SW_SEEN(p) ((sw.seen >> (p)) & 1u)
/* Once a pass, after the switch: log the callbacks and the milestones that first show, else a changed register word. */
static void sw_sample(void)
{
    u32 a[5], ms, fired = 0, rs, adr, en;
    if (!sw_open)
        return;
    ms = sw_ms();
    sw_words(a);
    rs = (a[2] >> 16) & 1u;
    adr = (a[4] >> 8) & 0x7fu;
    en = (a[4] >> 16) & 0x3fu;
    if (SW_SEEN(SW_FW_SWITCH)) {
        if (sw_dis_cnt != sw.dis) {
            sw.dis = sw_dis_cnt;
            sw_ov_ms = sw_cb_ms(sw_dis_at, ms);
            sw_ov_ps = sw_dis_ps;
            sw_ev(SW_DISCONNECT_CB, 0);
            fired = 1;
        }
        if (!rs) {
            sw.stopped = 1;
        } else if (sw.stopped && !SW_SEEN(SW_RS_UP)) {
            sw.rs_at = ms;
            sw_ev(SW_RS_UP, 0);
            fired = 1;
        }
        if (sw_con_cnt != sw.con) {
            sw.con = sw_con_cnt;
            if (!sw.reset_seen)
                sw.reset_seen = 1;
            sw_ov_ms = sw_cb_ms(sw_con_at, ms);
            sw_ov_ps = sw_con_ps;
            sw_ev(SW_CONNECT_CB, 0);
            fired = 1;
        }
        if (!sw.reset_seen && (a[2] & 0x40u) != 0u)
            sw.reset_seen = 2;
        if (!sw.reset_seen && adr == 0u)
            sw.reset_seen = 3;
        if ((en & 3u) == 0u)
            sw.en_cleared = 1;
        /* CONFIGURED first: ADDRESSED must be from an earlier pass */
        if (SW_SEEN(SW_ADDRESSED) && sw.en_cleared && (en & 3u) != 0u) {
            sw_ev(SW_CONFIGURED, 0);
            sw_ev(SW_DONE, 0);
            sw_open = 0;
            return;
        }
        if (SW_SEEN(SW_ADDRESSED) && ms - sw.addr_at >= SW_NOCONF_MS) {
            sw_ev(SW_DONE, 1); /* no configure seen */
            sw_open = 0;
            return;
        }
        if (SW_SEEN(SW_RS_UP) && sw.reset_seen && !SW_SEEN(SW_ADDRESSED) && adr != 0u) {
            sw.addr_at = ms;
            sw_ev(SW_ADDRESSED, sw.reset_seen);
            fired = 1;
        }
        if (SW_SEEN(SW_RS_UP) && (a[4] & 2u) != 0u)
            sw.sof = 1;
        if (SW_SEEN(SW_RS_UP) && !SW_SEEN(SW_ADDRESSED) && !sw.late &&
            ms - sw.rs_at >= SW_LATE_MS) {
            sw.late = 1;
            sw_ev(SW_ENUM_LATE, sw.sof); /* detail 1: SOF seen, no recovery */
            fired = 1;
        }
    }
    if (ms >= sw.win && ms - sw.win >= SW_WINDOW_MS) {
        sw_ev(SW_TIMEOUT, 0);
        if (sw.rec_n != 0u &&
            !sw.gave) { /* a recovery ran and the host never configured: the timeline ends with GIVEUP */
            sw.gave = 1;
            sw_ev(SW_GIVEUP, 0);
        }
        sw_open = 0;
        return;
    }
    if (!fired && (a[2] != sw.last[0] || a[3] != sw.last[1] || (a[4] & ~0xau) != sw.last[2]))
        sw_ev(SW_STATE, 0);
    sw.last[0] = a[2];
    sw.last[1] = a[3];
    sw.last[2] = a[4] & ~0xau;
    sw.last_fr = reg_rd(FRINDEX);
    sw.last_gpt = reg_rd(GPTIMER0CTL) & 0xffffffu;
    sw.clk = (a[4] >> 3) & 1u;
}

/* record a switch made now. Interrupts masked by the caller. */
static void sw_stamp(void)
{
    u32 per, now = to_peek(&per);
    if (to_sw_set && per == to_sw_per)
        usb_bulk_switch_gap_ms = to_elapsed_ms(to_sw_at, now, per);
    to_sw_set = 1;
    to_sw_at = now;
    to_sw_per = per;
    restart_due = 0; /* every switch connects the port again with entry 2 as it is now */
    usb_bulk_switches++;
    rec_armed = 1; /* a restart must follow (recovery_tick) */
    rec_at = now;
    rec_per = per;
    rec_count = 0;
}

/* ---- the detach hold and the recovery (finding 123) ----
 * The firmware's switch stops the controller (USBCMD.RS = 0: "clearing this bit causes a detach event", RM 24.3.4.1) and restarts it 30 ticks (1 s) later. With the
 * external ULPI PHY the host must see that detach. Hardware showed a host that did not (cycle 5: no disconnect, the old interface list kept, the device silent at its
 * address). After a stop the adapter therefore (1) holds the restart for 2 s (FW_HOLD_TICKS), (2) while the controller is stopped sets the PHY's function control to
 * non-driving without termination (ULPI_FC_DETACH, through ULPI_VIEWPORT, RM 24.3.4.12) so that the line is released whatever the controller left, and (3) writes the
 * value back FW_HOLD_TICKS - HOLD_RESTORE_TICKS ticks later, before the firmware sets RS again. (4) If the restart has not happened TO_FORCE_MS after a switch although VBUS is
 * present, recovery_tick calls the firmware's restart itself. Nothing here runs unless a switch of ours was made (hold_on, rec_armed), and the firmware's own menu
 * path is untouched. UB_ULPI_DETACH 0 turns the PHY writes off (the hold and the recovery stay). */
#ifndef UB_ULPI_DETACH
#define UB_ULPI_DETACH 0
#endif

/* One ULPI register access through the viewport. 0 done (`*rd` the read value), -1 refused (`usb_bulk_phy_why`): the interface is not in normal sync state and the
 * wake-up failed, or an earlier run (the firmware's own watchdog read of register 0, or a run of ours that did not finish) is still going after a short wait. A run is
 * never started while RUN is set, and a run of ours that does not finish is not cancelled: the next call finds RUN set, waits again, and refuses until it clears, so the
 * firmware's watchdog (below) does not see a hung run that we started and then abandon.
 * The watchdog (0x40006af8 on DT2 1.17) is a callback that the firmware queues on the USB timer (FUN_40006888 sets USBINTR.TIE0, bit 24, and the USB interrupt runs the
 * list), so it runs in interrupt context, and the stop clears USBINTR: while the controller is stopped it does not run at all. The interrupt mask held during one access keeps
 * it from starting a run between our check and our write while the controller is running. The mask covers one access, with a spin bound of ULPI_SPINS reads for each of
 * the three waits (a run takes a few ULPI clocks of 60 MHz), so a dead PHY clock stalls interrupts for at most about 3 ms. */
volatile u32
    usb_bulk_phy_why; /* why the last ULPI access was refused: 1 not in sync state, 2 an earlier run did not finish, 3 ours did not finish */

#define reg32(off) reg_rd(off)
#define vp_read() reg32(ULPI_VIEWPORT)

static i32 ulpi_xfer(u32 write, u32 reg, u32 data, u32 *rd)
{
    u32 i, v, sr = sr_mask();
    i32 r = -1;
    v = vp_read();
    if ((v & ULPI_SS) ==
        0u) { /* low power, serial or carkit state: the wake-up operation (RM 24.3.4.12) brings the interface back, a run without it is undefined */
        USB_REG(ULPI_VIEWPORT) = ULPI_WU; /* port 0 */
        v = vp_read();
        for (i = 0; i < ULPI_SPINS && (v & ULPI_WU) != 0u; i++)
            v = vp_read();
        if ((v & ULPI_WU) != 0u || (v & ULPI_SS) == 0u) {
            usb_bulk_phy_why = 1u;
            goto out;
        }
    }
    for (i = 0; i < ULPI_SPINS && (v & ULPI_RUN) != 0u; i++)
        v = vp_read();
    if ((v & ULPI_RUN) != 0u) {
        usb_bulk_phy_why = 2u;
        goto out;
    }
    USB_REG(ULPI_VIEWPORT) =
        ULPI_RUN | (write ? ULPI_RW : 0u) | (reg << 16) | (data & 0xffu); /* port 0 */
    v = vp_read();
    for (i = 0; i < ULPI_SPINS && (v & ULPI_RUN) != 0u; i++)
        v = vp_read();
    if ((v & ULPI_RUN) != 0u) {
        usb_bulk_phy_why = 3u;
        goto out;
    }
    *rd = (v >> 8) & 0xffu;
    if (reg == ULPI_FUNC_CTRL)
        sw_fc = write ? data & 0xffu : *rd;
    r = 0;
out:
    sr_restore(sr);
    return r;
}

/* Reset the PHY (function control bit 5, self-clearing; the PHY returns to its default state). Only for the recovery, with the controller stopped; the wait for the bit to
 * clear is bounded (20 reads). */
#define ULPI_FC_RESET 0x61u /* full speed, TermSelect 0, normal, Reset, SuspendM */
#if UB_ULPI_DETACH
static void phy_reset(void)
{
    u32 i, fc = 0, junk;
    if (ulpi_xfer(1, ULPI_FUNC_CTRL, ULPI_FC_RESET, &junk) != 0)
        return;
    for (i = 0; i < 20u; i++) {
        if (ulpi_xfer(0, ULPI_FUNC_CTRL, 0, &fc) != 0 || (fc & 0x20u) == 0u)
            break;
    }
}
#endif

/* The state of the hold. UI task only (hold_begin from the take-over tick, hold_service and recovery_tick from the tick). `hold_on`: a stop of ours waits for its
 * restart, or the PHY is still parked. `phy_parked`: the PHY holds ULPI_FC_DETACH and `phy_saved` is the function control to put back; `phy_tries`: failed attempts to put it back. */
#define PHY_TRIES_MAX 50u /* passes of the UI loop (about 0.4 s) */
static volatile u32 hold_on, phy_parked;
static u32 phy_saved, phy_tries;
static u32 hold_began,
    hold_park; /* the result of hold_begin for the timeline: the hold began; the park: 0 none, 1 done, 2 refused */
/* for the emulator test and the hardware log */
volatile u32 usb_bulk_hold_ticks, usb_bulk_phy_late, usb_bulk_phy_refused, usb_bulk_forced_restarts;

/* The park retry. `park_retry`: the park was refused and is tried again once a pass while the hold lasts; `park_tries`: attempts so far; `park_why`: usb_bulk_phy_why of the
 * first refusal; `park_saved`: `phy_saved` holds a value read in this hold (a retry after a write that did not finish may read ULPI_FC_DETACH back). */
#define ULPI_FC_STOCK 0x41u /* the function control the controller sets up itself */
static u32 park_retry, park_tries, park_why, park_saved;

/* PHY_ID, once per boot, right after a successful park (controller stopped, viewport idle: never while the controller runs, the firmware's viewport watchdog would reset it on a
 * hung run). Reads ULPI registers 0 to 3 (vendor and product ID). a2 = vendor (reg 1 << 8 | reg 0), a3 = product (reg 3 << 8 | reg 2), a4 the usual OTG word, a1 = 0. detail 0 ok;
 * a refused read tries again at the next park; after PID_TRIES_MAX refused tries detail 1 is logged once (the words hold what was read). UI task only. */
#define PID_TRIES_MAX 50u
static u32 pid_done, pid_tries;

static void phy_id_try(void)
{
    u32 a[5], r[4], i, bad = 0;
    if (pid_done)
        return;
    r[0] = r[1] = r[2] = r[3] = 0;
    for (i = 0; i < 4u && !bad; i++)
        bad = (u32)(ulpi_xfer(0, i, 0, &r[i]) != 0);
    if (bad && ++pid_tries < PID_TRIES_MAX)
        return;
    pid_done = 1;
    sw_words(a);
    a[0] = SW_PHY_ID | ((bad & 7u) << 13) | ((usb_bulk_switches & 0xffffu) << 16);
    a[1] = 0;
    a[2] = (r[1] << 8) | r[0];
    a[3] = (r[3] << 8) | r[2];
    dkrt_log_ev(SW_KIND, SW_FLAGS, a);
}

/* One park attempt: read the function control, write ULPI_FC_DETACH. 1 when parked. UI task, controller stopped. */
static u32 park_try(void)
{
    u32 fc = 0, back = 0;
    if (ulpi_xfer(0, ULPI_FUNC_CTRL, 0, &fc) != 0)
        return 0;
    if (fc != ULPI_FC_DETACH) {
        phy_saved = fc;
        park_saved = 1;
    } else if (!park_saved) {
        phy_saved = ULPI_FC_STOCK;
        park_saved = 1;
    }
    if (ulpi_xfer(1, ULPI_FUNC_CTRL, ULPI_FC_DETACH, &back) != 0)
        return 0;
    phy_parked = 1;
    phy_id_try();
    return 1;
}

/* PHY_BUSY (and the late PHY_PARK): word 2 = why (bits 0-7), tries (8-15), USBCMD.RS (16), last function control (24-31); PHY_BUSY word 3 = the raw ULPI_VIEWPORT */
static void sw_park_ev(u32 phase, u32 detail)
{
    sw_x_a2 = (park_why & 0xffu) | ((park_tries & 0xffu) << 8) |
              ((reg_rd(USBCMD) & USBCMD_RS) << 16) | (sw_fc << 24);
    sw_x_a3 = reg_rd(ULPI_VIEWPORT);
    sw_x_set = phase == SW_PHY_BUSY ? 3u : 1u;
    sw_ev(phase, detail);
    sw_x_set = 0;
}

/* Right after the firmware's stop: raise the counter, release the line. Interrupts on, UI task. */
static void hold_begin(void)
{
    u32 sr = sr_mask();
    volatile u32 *delay = (volatile u32 *)usb_restart_delay;
    u32 d = *delay, stopped = 0, began = 0;
    if (d != 0u &&
        d <= FW_STOP_TICKS) { /* the stop has just set it (the tick may have counted a few down) */
        began = 1;
        *delay = FW_HOLD_TICKS;
        usb_bulk_hold_ticks = FW_HOLD_TICKS;
        hold_on = 1;
        phy_parked = 0;
        phy_tries = 0;
        stopped = (reg32(USBCMD) & USBCMD_RS) == 0u;
    }
    sr_restore(sr);
    hold_began = began;
    hold_park = 0;
    if (began)
        sw.stopped |= stopped;
#if UB_ULPI_DETACH
    park_retry = 0;
    if (stopped) { /* the hold is 2 s: the accesses need no mask around the pair */
        park_saved = 0;
        park_tries = 1;
        if (park_try()) {
            hold_park = 1;
        } else {
            usb_bulk_phy_refused++;
            hold_park = 2;
            park_why = usb_bulk_phy_why;
            park_retry = 1; /* hold_service tries again once a pass */
        }
    }
#endif
}

/* Once a pass: put the PHY back before the firmware restarts, and end the hold. Interrupts on, UI task. With the controller stopped the saved value is written. If it
 * runs already (the restart came early, or the UI loop stalled past the last 100 ms) the controller has set the PHY up itself, so the function control is read and written
 * back only when it still holds our value: the device must never keep running with no termination. A failed access is retried every pass. */
static void hold_service(void)
{
    u32 sr, d, rs;
    if (!hold_on)
        return;
    sr = sr_mask();
    d = *(volatile u32 *)usb_restart_delay;
    rs = (reg32(USBCMD) & USBCMD_RS) != 0u;
    sr_restore(sr);
    if (park_retry) { /* one attempt a pass, not after the restore time */
        if (!rs && d > HOLD_RESTORE_TICKS && d <= FW_RESTART_TICKS) {
            park_tries++;
            if (park_try()) {
                park_retry = 0;
                sw_park_ev(SW_PHY_PARK, 2);
            }
        } else {
            park_retry = 0;
            sw_park_ev(SW_PHY_BUSY, 3); /* the hold ended without a park */
        }
    }
    if (phy_parked &&
        (rs || d <= HOLD_RESTORE_TICKS)) { /* d == 0: the restart did not run (no VBUS) */
        u32 fc = 0, junk = 0;
        i32 ok = 0;
        if (rs) {
            ok = ulpi_xfer(0, ULPI_FUNC_CTRL, 0, &fc);
            if (ok == 0 && fc == ULPI_FC_DETACH)
                ok = ulpi_xfer(1, ULPI_FUNC_CTRL, phy_saved, &junk);
            if (ok == 0)
                usb_bulk_phy_late++;
        } else {
            ok = ulpi_xfer(1, ULPI_FUNC_CTRL, phy_saved, &junk);
        }
        if (ok == 0) {
            phy_parked = 0;
            sw_ev(SW_PHY_RESTORE, 0);
        } else if (++phy_tries >= PHY_TRIES_MAX) {
            phy_parked = 0;
            usb_bulk_phy_refused++;
            sw_ev(SW_PHY_RESTORE, 1);
            UB_NOTE("ub.phy.restore.no", phy_tries);
        }
    }
    if (!phy_parked && (d == 0u || d > FW_RESTART_TICKS))
        hold_on = 0;
}

/* Once a pass: a switch of ours was made, the controller is still stopped, and VBUS has been present for TO_FORCE_MS with the firmware not waiting to restart it any more
 * (the restart was lost, or ran without effect): reset the PHY and call the firmware's restart, at most RECOVERY_MAX times for one switch, then give up (logged). The window
 * restarts whenever VBUS is absent, so a plug-in starts a new one and the firmware's own BSV restart goes first. The recovery ends (disarms) when the controller runs or the
 * active port is no longer the target of our switch (a menu change, Overbridge, the safe mode). Hardware (finding 123) left the device off the bus until a power cycle after
 * a failed switch. */
#define RECOVERY_MAX 3u
static void recovery_tick(void)
{
    u32 sr, per, now, act = 0, gave = 0, quiet, vbus, late;
    if (!rec_armed)
        return;
    sr = sr_mask();
    if ((reg32(USBCMD) & USBCMD_RS) != 0u || *(volatile u32 *)usb_active_port != rec_target) {
        rec_armed = 0; /* the controller runs, or the mode is not ours any more */
        sr_restore(sr);
        return;
    }
    now = to_peek(&per);
    vbus = (reg32(OTGSC) & OTGSC_BSV) != 0u;
    if (per != rec_per || !vbus) { /* the clock source changed, or no VBUS: count again from now */
        rec_per = per;
        rec_at = now;
    } else {
        quiet = !fw_restart_pending() && !hold_on;
        late = to_elapsed_ms(rec_at, now, per) >= TO_FORCE_MS;
        if (quiet && late) {
            rec_at = now;
            if (rec_count >= RECOVERY_MAX) {
                rec_armed = 0;
                gave = 1;
                UB_NOTE("ub.rstart.giveup", rec_count);
            } else {
                rec_count++;
                usb_bulk_forced_restarts++;
                act = 1;
            }
        }
    }
    sr_restore(sr);
    if (gave)
        sw_ev(SW_GIVEUP, 0);
    if (act) {
        sw_ev(SW_RECOVERY, rec_count);
#if UB_ULPI_DETACH
        phy_reset(); /* a PHY that a stop left in a state the restart does not undo */
#endif
        UB_NOTE("ub.forced.rstart", usb_bulk_forced_restarts);
        usb_fw_restart();
    }
}

/* EP0 or EP1 IN has data primed for the host (a SysEx reply, a control answer), or one of our own EP2 IN sends is queued: a stop now would cut it. */
static u32 tx_in_flight(void)
{
    u32 i;
    if (((reg_rd(ENDPTSTAT) | reg_rd(ENDPTPRIME)) & EP_IN_CTRL_MIDI) != 0u)
        return 1;
    for (i = 0; i < TX_SLOTS; i++)
        if (tx_state[i] == T_QUEUED)
            return 1;
    return 0;
}

/* the take-over ends (the decision for a release has run: its switch follows) */
static void to_end(void)
{
    free_req = 1; /* the tick frees the buffers once nothing holds them (buffers_free) */
    taken = 0;
    to_req = 0;
    to_gone = 0;
    to_sus = 0;
}

/* the take-over ends without a switch (the user changed the mode, or the switch was never needed); the tick logs DROPPED */
static void to_drop(void)
{
    if (sw_open || sw_pend)
        sw_drop = 1;
    to_end();
}

/* The take-over's decision for this tick: updates the intervals and the request, and returns the port to switch to, or 0 for no switch. `port` is the active
 * port (or the target of a claimed switch), `guard` the result of sw_guard_open, `busy` tx_in_flight (a switch waits TO_BUSY_MS for it). A target equal to `port` is a request that needs no switch (it is dropped).
 * Interrupts masked by the caller. */
static u32 takeover_decide(u32 now, u32 per_s, u32 port, u32 guard, u32 busy)
{
    if (per_s != to_per_s) { /* the clock source changed: restart the intervals */
        to_per_s = per_s;
        to_act_at = now;
        to_due_set = 0;
        to_sus = 0;
    }
    if (to_active) {
        to_active = 0;
        to_act_at = now;
    }
    /* once switched, the active port stays USB MIDI until we switch back: any other value is the user's choice and wins */
    if (to_req != 1u && port != PORT_ENTRY) {
        to_drop();
        return 0;
    }
    if (to_req == 0u &&
        !guard) { /* our own switch is settling: its disconnects and the bus suspend are not a host leaving */
        to_gone = 0;
        to_sus = 0;
        return 0;
    }
    if (to_req ==
        0u) { /* the automatic give-back: the host left, no activity for TO_IDLE_MS, or the bus stayed suspended */
        if (to_gone) {
            to_gone = 0;
            to_req = 2;
        } else if (to_elapsed_ms(to_act_at, now, per_s) >= TO_IDLE_MS) {
            to_req = 2;
        } else if ((reg_rd(PORTSC) & PORTSC_SUSP) != 0u &&
                   to_elapsed_ms(to_act_at, now, per_s) >= TO_QUIET_MS) {
            if (!to_sus) {
                to_sus = 1;
                to_sus_at = now;
            } else if (to_elapsed_ms(to_sus_at, now, per_s) >= TO_SUSPEND_MS) {
                to_req = 2;
            }
        } else {
            to_sus = 0;
        }
        if (to_req == 2u) { /* no delay */
            if (to_orig != PORT_ENTRY)
                sw_pend = 2;
            to_due_ms = 0;
            to_due_set = 0;
        }
    }
    if (to_req == 0u)
        return 0;
    if (!to_due_set) {
        to_due_set = 1;
        to_due_at = now;
    }
    if (to_elapsed_ms(to_due_at, now, per_s) < to_due_ms ||
        !guard) /* a request waits for its delay and for the guard: one switch at a time */
        return 0;
    if (busy && to_elapsed_ms(to_due_at, now, per_s) < to_due_ms + TO_BUSY_MS)
        return 0; /* a reply is still going out: wait for it, but not for ever (a MIDI IN the host never reads) */
    if (to_req == 1u) {
        if (port != to_orig) { /* the mode changed before the switch ran: nothing to take over */
            to_drop();
            return 0;
        }
        to_req = 0;
        to_active = 1; /* the switch counts as activity */
        to_sus = 0;
        return PORT_ENTRY;
    }
    to_end();
    return to_orig;
}

/* Called from usb_bulk_tick, the UI task: decide under the mask, then run the switch. */
static void takeover_tick(void)
{
    u32 per_s, now, sr, port, target, guard, busy, wait = 0;
    if (!taken)
        return;
    /* the switch to USB MIDI connects port 2 from the USB interrupt, which primes the buffers: they must be there before. A failure ends the take-over (ST_NOMEM) */
    if (to_req == 1u && !live) {
        i32 r = buffers_alloc();
        if (r == E_NOMEM) {
            sr = sr_mask();
            to_drop();
            sr_restore(sr);
        }
        if (r != 0)
            return; /* E_STATE: the job is allocating; the next pass looks again */
    }
    now = to_clock(&per_s);
    sr = sr_mask();
    port = to_pend_port != 0u ? to_pend_port : *(volatile u32 *)usb_active_port;
    guard = sw_guard_open();
    busy = tx_in_flight();
    target = takeover_decide(now, per_s, port, guard, busy);
    if (target == 0u && to_req != 0u && to_due_set &&
        to_elapsed_ms(to_due_at, now, per_s) >= to_due_ms)
        wait = !guard ? 1u : busy ? 2u : 0u; /* the request is due and waits */
    if (target != 0u && target != port) {
        /* claimed under the mask: from here a takeover() or giveback() on the MIDI task sees the new state, and any further request waits for the guard */
        sw_stamp();
        rec_target = target;
        to_pend_port = target;
    }
    sr_restore(sr);
    sw_pend_poll(); /* an automatic give-back is accepted in the decision above */
    if (sw_drop) {
        sw_drop = 0;
        sw_ev(SW_DROPPED, 0);
        sw_open = 0;
    }
    if (wait != 0u && !sw.waited) {
        sw.waited = 1;
        sw_ev(SW_WAIT, wait);
    }
    if (target != 0u && target != port) {
        u32 a[5];
        usb_fw_mode_switch(
            target); /* interrupts on, task context: the firmware's own menu action does the same */
        to_pend_port = 0; /* the firmware has written the active port */
        sw.hold_ms =
            sw_ms(); /* log nothing until the PHY is parked: stamp the time and the words */
        sw_words(a);
        sw.hold_w[0] = a[2];
        sw.hold_w[1] = a[3];
        sw.hold_w[2] = a[4];
        sw.hold_ov = 1;
        hold_begin();
        sw_ev(SW_FW_SWITCH, 0);
        if (hold_began)
            sw_ev(SW_HOLD_BEGIN, 0);
        if (hold_park != 0u)
            sw_ev(SW_PHY_PARK, hold_park - 1u);
        if (hold_park == 2u)
            sw_park_ev(SW_PHY_BUSY, 0); /* the first refusal of this switch */
        sw.hold_ov = 0;
    }
}

/* ---- the ENUM_LATE watchdog ----
 * ENUM_LATE (the controller runs, but no host addressed the device 3 s after RS_UP) starts a recovery, at most WD_MAX a switch: RECOVERY is logged, then the firmware's
 * stop with restart type 2 (usb_fw_reset_stop), under the interrupt mask. Its tick resets the controller (USBCMD.RST) 30 ticks later, initialises it and starts it if VBUS is
 * present: this is the firmware's own reset of a hung USB block. Nothing else is done (no PHY access, no usb_fw_restart: that sets restart type 1, a plain restart). A plain
 * stop and restart did not heal a link that the PHY had lost (hardware, finding 123). The timeline window goes on: its milestones and ENUM_LATE can fire again, and its end moves
 * to SW_WINDOW_MS after the call. A third ENUM_LATE logs GIVEUP. It claims the switch guard like a switch and does not run while a switch, a request, a hold, a restart of the
 * firmware or a forced restart is pending, or without VBUS. The firmware's init may rewrite entry 2 of the port table: once RS shows 1 again the hooks are checked. UI task only. */
#define WD_MAX 2u
#define WD_BOOT_MAX                                                                                \
    2u /* resets a boot (usb_bulk_enum_recoveries): the firmware's init registers its USB tick in one of 16 shared timer slots each time and never frees it */
volatile u32 usb_bulk_enum_recoveries;
static u32 wd_rehook; /* a reset was made: check the port entry once RS shows 1 again */

/* The reset was made: the milestones and ENUM_LATE can fire again, the window ends SW_WINDOW_MS from now. */
static void wd_rearm(void)
{
    sw.win = sw_ms();
    sw.seen &= ~((1u << SW_RS_UP) | (1u << SW_ADDRESSED) | (1u << SW_CONFIGURED));
    sw.late = sw.reset_seen = sw.en_cleared = sw.addr_at = sw.sof = 0;
    sw.stopped = 1; /* RS_UP is logged when RS shows 1 again */
}

/* The firmware's init after the reset may rewrite entry 2 of the port table. Once RS shows 1 again (the firmware started the controller) the four words are compared with
 * ours and written back if they differ (the same words usb_bulk_enable writes); the note says which differed (bit n = word n). Nothing is checked while bulk is not enabled. */
static void wd_rehook_check(void)
{
    u32 sr, diff = 0;
    u32 *e = (u32 *)(usb_port_table + PORT_ENTRY * PORT_ENTRY_BYTES);
    if ((reg_rd(USBCMD) & USBCMD_RS) == 0u)
        return;
    sr = sr_mask();
    wd_rehook = 0;
    if (enabled) {
        diff = (u32)(e[0] != (u32)usb_bulk_connect) |
               ((u32)(e[1] != (u32)usb_bulk_disconnect) << 1) |
               ((u32)(e[3] != (u32)desc_buf()) << 3) | ((u32)(e[4] != DESC_BYTES) << 4);
        if (diff != 0u) {
            e[3] = (u32)desc_buf();
            e[4] = DESC_BYTES;
            e[0] = (u32)usb_bulk_connect;
            e[1] = (u32)usb_bulk_disconnect;
        }
    }
    sr_restore(sr);
    if (diff != 0u)
        UB_NOTE("ub.rehook", diff);
}

/* After a reset the controller may stay stopped with VBUS not seen (OTGSC.BSV 0) and the USB timer (GPT0) not counting: the firmware's init clears MISCCR.USBSRC, so the
 * PHY's 60 MHz output drives the controller clock, and a PHY that stopped its clock gives no BSV. The ULPI wake-up operation (RM 24.3.4.12: WU set, RUN clear) puts the
 * interface into normal operation and re-enables the clock. Once a recovery, WD_WAKE_MS after the reset call, with RS 0, BSV 0 and no timer movement: one bounded
 * poll (WD_WAKE_POLL_MS, interrupts not masked). After a wake that worked: if BSV shows and RS stays 0 for WD_START_MS, usb_fw_restart (the firmware's own BSV interrupt
 * may start the controller first; RS 1 ends the wait). `wd_wk`: 0 idle, 1 waiting for the wake-up, 2 waiting for BSV and the start. UI task only. */
#define WD_WAKE_MS 1500u
#define WD_WAKE_POLL_MS 15u
#define WD_BSV_WAIT_MS 2000u
#define WD_START_MS 1000u
static u32 wd_wk, wd_wk_at, wd_wk_no, wd_wk_rec, wd_wk_bsv, wd_wk_bsv_at;

/* PHY_WAKE: word 2 = ULPI_VIEWPORT after the poll, word 3 = ms polled */
static void wd_wake_ev(u32 detail, u32 vp, u32 polled)
{
    sw_x_a2 = vp;
    sw_x_a3 = polled;
    sw_x_set = 3u;
    sw_ev(SW_PHY_WAKE, detail);
    sw_x_set = 0;
}

static void wd_wake_tick(void)
{
    u32 ms, v, i, t0, el = 0;
    if (wd_wk == 0u)
        return;
    if (!sw_open || sw.no != wd_wk_no || sw.rec_n != wd_wk_rec ||
        (reg_rd(USBCMD) & USBCMD_RS) != 0u) {
        wd_wk = 0;
        return;
    }
    ms = sw_ms();
    if (ms < wd_wk_at)
        return;
    if (wd_wk == 1u) {
        if (ms - wd_wk_at < WD_WAKE_MS || (reg_rd(OTGSC) & OTGSC_BSV) != 0u || sw.clk)
            return;
        v = reg_rd(ULPI_VIEWPORT);
        if ((v & ULPI_RUN) != 0u) {
            wd_wk = 0;
            wd_wake_ev(2, v, 0);
            return;
        }
        USB_REG(ULPI_VIEWPORT) = ULPI_WU; /* port 0, RUN clear */
        t0 = dkrt_blocks();
        for (i = 0; i < ULPI_SPINS * 20u; i++) {
            v = reg_rd(ULPI_VIEWPORT);
            if ((v & ULPI_WU) == 0u)
                break;
            if (t0 != 0u) {
                el = to_elapsed_ms(t0, dkrt_blocks(), 1500u);
                if (el >= WD_WAKE_POLL_MS)
                    break;
            }
        }
        if (t0 != 0u)
            el = to_elapsed_ms(t0, dkrt_blocks(), 1500u);
        if ((v & ULPI_WU) == 0u) {
            wd_wk = 2;
            wd_wk_at = sw_ms();
            wd_wk_bsv = 0;
            wd_wake_ev(0, v, el);
        } else {
            wd_wk = 0;
            wd_wake_ev(1, v, el);
        }
        return;
    }
    if (ms - wd_wk_at > WD_BSV_WAIT_MS && !wd_wk_bsv) {
        wd_wk = 0;
        return;
    }
    if ((reg_rd(OTGSC) & OTGSC_BSV) == 0u)
        return;
    if (!wd_wk_bsv) {
        wd_wk_bsv = 1;
        wd_wk_bsv_at = ms;
        return;
    }
    if (ms - wd_wk_bsv_at >= WD_START_MS && *(volatile u32 *)usb_restart_delay == 0u) {
        wd_wk = 0;
        usb_fw_restart();
    }
}

static void wd_tick(void)
{
    u32 sr, per, now, go, take = 0;
    if (wd_rehook)
        wd_rehook_check();
    wd_wake_tick();
    if (!sw_open || sw.fw || !sw.late || SW_SEEN(SW_ADDRESSED) || sw.gave)
        return;
    if (sw.rec_n >= WD_MAX) {
        sw.gave = 1;
        sw_ev(SW_GIVEUP, 0);
        return;
    }
    if (sw.sof) /* the link is alive (SOF seen since RS_UP): ENUM_LATE detail 1, no recovery */
        return;
    if (usb_bulk_enum_recoveries >=
        WD_BOOT_MAX) { /* every reset leaks a firmware timer slot: no more this boot */
        static u32 cap_noted;
        sw.gave = 1;
        sw_ev(SW_GIVEUP, 0);
        if (!cap_noted) {
            cap_noted = 1;
            UB_NOTE("ub.reset.cap", usb_bulk_enum_recoveries);
        }
        return;
    }
    sr = sr_mask();
    go = (reg_rd(USBCMD) & USBCMD_RS) != 0u && (reg_rd(OTGSC) & OTGSC_BSV) != 0u && !rec_armed &&
         !hold_on && to_req == 0u && !sw_pend && *(volatile u32 *)usb_active_port == sw.target &&
         *(volatile u32 *)usb_restart_delay == 0u && !tx_in_flight() && sw_guard_open();
    /* Hardware experiment: no reset for a failed take-over. The reset zero-fills the USB SRAM queue heads and the dTD pool, which the bulk endpoint (port entry 2) uses;
     * take-over support needs the adapter to rebuild its bulk state first. Only a failed release (target is the stock port) is reset. */
    if (go && sw.target == PORT_ENTRY) {
        go = 0;
        take = 1;
    }
    if (go) { /* claim the guard like a switch (no switch number); the firmware restarts the controller (recovery_tick is not armed) */
        now = to_peek(&per);
        to_sw_set = 1;
        to_sw_at = now;
        to_sw_per = per;
        to_pend_port = sw.target;
        to_active = 1;
    }
    sr_restore(sr);
    if (take) {
        static u32 take_noted;
        sw.gave = 1;
        sw_ev(SW_GIVEUP, 0);
        if (!take_noted) {
            take_noted = 1;
            UB_NOTE("ub.reset.take", 0);
        }
        return;
    }
    if (!go)
        return;
    sw.rec_n++;
    sw_ev(SW_RECOVERY, sw.rec_n); /* before the reset */
    usb_bulk_enum_recoveries++;
    sr = sr_mask();
    to_quiet = 1; /* the stop runs our disconnect callback: not a host leaving */
    usb_fw_reset_stop();
    to_quiet = 0;
    to_pend_port = 0;
    sr_restore(sr);
    wd_rehook = 1;
    wd_rearm();
    wd_wk = 1;
    wd_wk_at = sw.win;
    wd_wk_no = sw.no;
    wd_wk_rec = sw.rec_n;
}

/* The job is woken every HK_WAKE_MS while the interface is enabled: the module has no timer of its own (a retry of usb_bulk_enable while the mode switch settles,
 * a check for an idle session). */
#define HK_WAKE_MS 250u
static u32 hk_set, hk_at, hk_per, hk_passes;

static void housekeeping_wake(void)
{
    u32 per = 1500u, now = dkrt_blocks();
    if (io_job <= 0)
        return;
    if (now == 0u) { /* no block clock: count the passes of the UI loop (about 120 a second) */
        per = 120u;
        now = ++hk_passes;
    }
    if (hk_set && per == hk_per && to_elapsed_ms(hk_at, now, per) < HK_WAKE_MS)
        return;
    hk_set = 1;
    hk_at = now;
    hk_per = per;
    wake();
}

/* once a pass of the UI loop (dkrt_poll_add): the take-over, then retry what the engine refused */
void usb_bulk_tick(void)
{
    u32 sr, again;
    sw_pend_poll();
    hold_service();
    recovery_tick();
    takeover_tick();
    sw_fw_poll();
    sw_sample();
    wd_tick();
    if (free_req && !taken)
        buffers_free();
    if (!enabled)
        return;
    housekeeping_wake();
    sr = sr_mask();
    if (need_prime)
        prime_all();
    again = tx_retry;
    tx_retry = 0;
    sr_restore(sr);
    if (again)
        wake();
}

/* ---- the interface for modules ---- */

/* The stock callback of the entry must be the firmware's own: `moveq #1,d0; move.l d0,4(sp); bra.w FUN_400043e2`, and the branch lands on the
 * connect body of the anchor. Anything else is a firmware this adapter has not been checked against. */
static int stock_connect_ok(u32 fn)
{
    const u8 *c = (const u8 *)fn;
    i32 disp;
    if (c[0] != 0x70 || c[1] != 0x01 || c[2] != 0x2f || c[3] != 0x40 || c[4] != 0x00 ||
        c[5] != 0x04 || c[6] != 0x60 || c[7] != 0x00)
        return 0;
    disp = (i32)(short)((c[8] << 8) | c[9]);
    return fn + 8u + (u32)disp == (u32)usb_fw_connect_body; /* bra.w: from the displacement word */
}

/* Put the tick on the runtime's list (UI task or MIDI task). The runtime's dkrt_poll_add is idempotent (a function that is in the table returns OK) but does an unlocked
 * scan and store on a 4-entry table: it is called under the mask here, and once only, so the UI loop (which reads the table, a word at a time) never sees a half write. */
static u32 tick_added;

static i32 tick_register(void)
{
    u32 sr;
    i32 r = 0;
    if (tick_added)
        return 0;
    sr = sr_mask();
    if (!tick_added) {
        r = dkrt_poll_add((u32)usb_bulk_tick);
        if (r == 0)
            tick_added = 1;
    }
    sr_restore(sr);
    return r;
}

/* Make entry 2 (USB MIDI) ours: the 124-byte descriptor, our connect and disconnect callbacks, the buffers. Writes only the port table entry and our own state,
 * so it is safe whatever the active port is: the firmware reads the entry when it connects. `restart_due`: the device is in USB MIDI mode and no switch is
 * settling, so the host has seen the stock descriptor and the caller must re-enumerate; in any other case the next connect of port 2 (a switch that is
 * settling, or a later one) already uses the new entry. */
static i32 enable_prepare(i32 io_id)
{
    u32 *e = (u32 *)(usb_port_table + PORT_ENTRY * PORT_ENTRY_BYTES);
    const u8 *cfg = (const u8 *)e[3];
    u32 sr, i;
    u8 *d;
    /* the descriptor must lie in the cached DDR window: the alias is that window plus a constant (the buffers are checked when they are allocated) */
    if ((u32)desc_mem < usb_ddr_base ||
        (u32)desc_mem + sizeof desc_mem > usb_ddr_base + usb_ddr_bytes)
        return E_CONTRACT;
    /* the stock entry, byte for byte (the firmware writes wTotalLength into its own array at run time, FUN_4000677a, through absolute addresses and not through
     * this table: bytes 2 and 3 are free) */
    if (e[4] != STOCK_CONFIG_BYTES || e[9] != 1u || e[1] != (u32)usb_fw_disconnect ||
        !stock_connect_ok(e[0]))
        return E_CONTRACT;
    for (i = 0; i < STOCK_CONFIG_BYTES; i++)
        if (i != 2u && i != 3u && cfg[i] != stock_config[i])
            return E_CONTRACT;
    if (tick_register() != 0)
        return E_NOMEM;
    /* the descriptor is bss: the installer may have zeroed it through the data cache, and a dirty line written back later would overwrite the descriptor
     * below. Push and invalidate every line first (the alias is another address for the same memory, to the cache), then write through the alias only. */
    for (i = 0; i < sizeof desc_mem; i += 16u)
        __asm__ volatile("cpushl %%bc,(%0)" : : "a"((u32)desc_mem + i) : "memory");
    d = desc_buf();
    for (i = 0; i < STOCK_CONFIG_BYTES; i++)
        d[i] = cfg[i];
    for (i = 0; i < 23u; i++)
        d[STOCK_CONFIG_BYTES + i] = bulk_interface[i];
    d[2] = DESC_BYTES; /* wTotalLength, little endian */
    d[3] = 0;
    d[4] = 3; /* bNumInterfaces */
    qh_ready = 0;
    io_job = io_id;
    sr = sr_mask();
    restart_due = usb_midi_mode() && sw_guard_open();
    stock_connect = (void (*)(u32))e[0];
    e[3] = (u32)d;
    e[4] = DESC_BYTES;
    e[0] = (u32)usb_bulk_connect;
    e[1] = (u32)usb_bulk_disconnect;
    enabled = 1;
    sr_restore(sr);
    return 0;
}

/* The caller (the module's I/O job) calls this until it returns 0. The first call prepares entry 2 whatever the mode (see enable_prepare), so a take-over
 * from USB AUDIO/MIDI enumerates once, with the bulk interface already in the descriptor. E_STATE: the device is not in USB MIDI mode, or a switch is
 * settling (the firmware restarts the device a second after it, and the guard keeps a second stop out); call again later. */
i32 usb_bulk_enable(i32 io_id)
{
    u32 sr, ok;
    i32 r;
    if (io_job > 0 && io_job != io_id)
        return E_BUSY; /* one owner */
    if (!enabled) {
        r = enable_prepare(io_id);
        if (r != 0)
            return r;
    }
    io_job = io_id;
    /* the bulk pair lives in the USB MIDI configuration; USB AUDIO/MIDI uses EP2 IN for the audio clock feedback
     * (checked also when the interface is enabled already, so a caller never sees success outside USB MIDI mode). With core/usb-bulk-audio, USB AUDIO/MIDI has the OUT-only
     * interface 5 (EP2 OUT): enable then allocates the ring and returns 0 with no re-enumeration. */
    if (!bulk_port())
        return E_STATE;
    /* The device is in USB MIDI mode: the buffers are needed from the next connect on. They are allocated here, not in the AUDIO/MIDI call that only prepared the
     * entry (a take-over allocates them in the tick before its switch). A device that connected before the buffers existed (the user switched mode, or the
     * buffers were freed after a take-over) has EP2 up already: buffers_alloc queues them, no re-enumeration. A switch that is settling is refused before
     * anything is allocated. A failure (E_NOMEM, ST_NOMEM) changes nothing. */
    sr = sr_mask();
    ok = sw_guard_open();
    sr_restore(sr);
    if (!ok)
        return E_STATE;
    r = buffers_alloc();
    if (r != 0)
        return r;
    free_req =
        0; /* an enable is a new request for the buffers: a free asked for by an earlier disable or take-over end must not take them back (the tick would free
                   * the block this call has just allocated) */
    if (usb_p5_mode())
        return 0; /* the device enumerated with interface 5 at boot: nothing to switch or restart */
    /* a switch is settling: the caller retries. Also when enabled already, so a caller never sees success in the middle of one */
    sr = sr_mask();
    ok = sw_guard_open();
    if (ok && restart_due) {
        restart_due = 0;
        sw_stamp(); /* the stop and restart below are a switch */
        rec_target = PORT_ENTRY;
        to_quiet = 1; /* the stop runs our disconnect callback: not a host leaving */
    } else if (ok) {
        sr_restore(sr);
        return 0;
    }
    sr_restore(sr);
    if (!ok)
        return E_STATE;
    /* the firmware's own USB mode switch (FUN_40006c16 does the same from the task context, interrupts on): stop (calls the disconnect callback and
     * starts a delay), restart (the tick restarts the device when the delay has run); the host sees the device go and come back with interface 2 */
    usb_fw_stop();
    to_quiet =
        0; /* only the stop above is our own: a real unplug from here on counts (after the guard) */
    usb_fw_restart();
    to_active = 1;
    return 0;
}

/* the job that enabled the interface goes away (the module's fini): no more wakes. The table stays as it is until the next boot. */
i32 usb_bulk_disable(i32 io_id)
{
    u32 sr, i;
    if (io_id <= 0 || io_job != io_id)
        return E_STATE;
    sr = sr_mask();
    io_job = 0;
    /* the job is going away (the runtime waits for its running step before fini, then cancels it): what it still holds is released, so the free can happen */
    for (i = 0; i < rx_slots; i++)
        if (rx_state[i] == B_TAKEN)
            rx_state[i] = B_FREE;
    for (i = 0; i < TX_SLOTS; i++)
        if (tx_state[i] == T_RESERVED)
            tx_state[i] = T_FREE;
    need_prime = 1;
    free_req = 1; /* the tick frees the buffers (once no take-over is on) */
    sr_restore(sr);
    return 0;
}

i32 usb_bulk_take(u8 **buf, u32 *len)
{
    u32 sr = sr_mask();
    i32 r = 0;
    if (need_prime)
        prime_all();
    if (order_count != 0) {
        u32 i = order[order_head];
        order_head = (order_head + 1u) % RX_MAX;
        order_count--;
        rx_state[i] = B_TAKEN;
        *buf = rx_buf(i);
        *len = rx_len[i];
        r = 1;
    }
    sr_restore(sr);
#if UB_CACHED_RX
    if (r ==
        1) { /* the buffer is ours (B_TAKEN, the DMA is done): invalidate with interrupts on, then hand out the cached address (rules 2 and 3) */
        u32 a = (u32)*buf - usb_alias_delta, n = (*len + 15u) & ~15u, k;
        for (k = 0; k < n; k += 16u)
            __asm__ volatile("cpushl %%dc,(%0)" : : "a"(a + k) : "memory");
        usb_bulk_rx_inval_lines += n / 16u;
        *buf = (u8 *)a;
    }
#endif
    return r;
}

i32 usb_bulk_release(u8 *buf)
{
    u32 sr = sr_mask();
    i32 i = rx_index(buf); /* -1 when the buffers are gone */
    i32 r = E_STATE;
    if (i >= 0 && rx_state[i] == B_TAKEN) {
        rx_state[i] = B_FREE;
        prime_all();
        r = 0;
    }
    sr_restore(sr);
    return r;
}

/* A slot is reserved under the mask, the bytes are copied with interrupts on (16 KiB is long), and the dTD is queued under the mask again. */
i32 usb_bulk_send(const u8 *buf, u32 len)
{
    u32 sr, i, k, before;
    u8 *p;
    i32 slot = -1, r;
    if (len == 0u || len > TX_BYTES)
        return E_LENGTH;
    sr = sr_mask();
    if (!live || !connected || stopping || !usb_midi_mode()) {
        sr_restore(sr);
        return E_STATE;
    }
    if (need_prime)
        prime_all();
    for (i = 0; i < TX_SLOTS; i++) {
        if (tx_state[i] == T_FREE) {
            tx_state[i] = T_RESERVED;
            slot = (i32)i;
            p = tx_buf(
                i); /* under the mask: the slot is reserved, so the buffers cannot be freed under the copy */
            break;
        }
    }
    before = resets;
    sr_restore(sr);
    if (slot < 0)
        return E_BUSY; /* both slots are in flight: their completion wakes the job */
    for (k = 0; k < len; k++)
        p[k] = buf[k];
    sr = sr_mask();
    if (!live || !connected || stopping || resets != before) {
        tx_state[slot] =
            T_FREE; /* the bus was reset while copying (ep2_stop leaves a reserved slot to us) */
        r = E_STATE;
    } else if (queue_transfer(IDX_IN, p, len, (void *)usb_bulk_tx_done, (void *)usb_bulk_cancel) !=
               0) {
        tx_state[slot] = T_FREE;
        tx_retry = 1;
        r = E_AGAIN;
    } else {
        tx_state[slot] = T_QUEUED;
        UB_LOG("ub.tx.queue", len | ((u32)slot << 24));
        r = 0;
    }
    sr_restore(sr);
    return r;
}

/* Take the USB port over for the bulk link, or renew a take-over (host activity for the watchdog). In USB AUDIO/MIDI (port 5) it records the original port and the
 * tick switches to USB MIDI about TO_DELAY_MS later; in USB MIDI it succeeds without switching. Refused (E_STATE) in Overbridge (port 6) and any other port.
 * A pending give-back is cancelled. Callable from the MIDI task (a command handler): it only sets state; the switch runs in the tick.
 * The caller then calls usb_bulk_enable until it stops returning E_STATE. */
i32 usb_bulk_takeover(void)
{
    u32 sr = sr_mask();
    u32 port = to_pend_port != 0u
                   ? to_pend_port
                   : *(volatile u32 *)usb_active_port; /* the port a claimed switch leaves */
    i32 r = 0;
    if (taken) {
        if (to_req == 2u)
            to_req = 0;
        to_active = 1;
    } else if (port != PORT_ENTRY && port != PORT_AUDIO) {
        r = E_STATE; /* Overbridge (6) or a service mode */
    } else {
        taken = 1;
        free_req = 0; /* a new session: keep the buffers if they are still there */
        to_orig = port;
        to_gone = 0;
        to_sus = 0;
        to_active = 1;
        to_due_set = 0;
        to_due_ms = TO_DELAY_MS;
        to_req = port == PORT_AUDIO ? 1u : 0u;
        if (port == PORT_AUDIO)
            sw_pend = 1;
    }
    sr_restore(sr);
    /* the tick must run while bulk is not enabled: add it here (a second add is a no-op; the slot is kept for good, the tick is cheap while nothing is taken) */
    if (r == 0 && tick_register() != 0) {
        sr = sr_mask();
        to_drop();
        sr_restore(sr);
        return E_NOMEM;
    }
    return r;
}

/* End a take-over: the tick switches back to the remembered port about TO_DELAY_MS later. Nothing taken: 0. A switch that has not run yet is dropped. */
i32 usb_bulk_giveback(void)
{
    u32 sr = sr_mask();
    if (taken) {
        if (to_req == 1u) {
            to_drop(); /* the switch never ran */
        } else {
            if (to_req != 2u && to_orig != PORT_ENTRY)
                sw_pend = 2;
            to_req = 2;
            to_due_set = 0;
            to_due_ms = TO_DELAY_MS;
        }
    }
    sr_restore(sr);
    return 0;
}

u32 usb_bulk_state(void)
{
    u32 sr = sr_mask();
    u32 i, tx = 0;
    u32 st = (enabled ? ST_ENABLED : 0u) | (connected ? ST_CONNECTED : 0u);
    /* bit 2: a live read of the active port, so a mode change after enable shows too; set when the port has no bulk route (not USB MIDI, not an attached port 5) */
    if (!bulk_port())
        st |= ST_WRONG_MODE;
    if (taken)
        st |= ST_TAKEN;
    if (usb_p5_mode())
        st |= ST_OUT_ONLY;
    if (live)
        st |= ST_LIVE;
    if (nomem)
        st |= ST_NOMEM;
    for (i = 0; i < TX_SLOTS; i++)
        tx += tx_state[i] != T_FREE ? 1u : 0u;
    st |= (order_count & 0xffu) << 8;
    st |= (tx & 0xffu) << 16;
    st |= (resets & 0xffu) << 24;
    sr_restore(sr);
    return st;
}

/* How many 64 KiB frames of the usb-fast protocol the receive ring takes before the controller NAKs: a frame (header, offset and 64 KiB) spans five 16 KiB slots, the last
 * one short. */
#define RING_SLOTS_PER_FRAME 5u

/* The run-time ring switch and the receive window. `set` below 0 only reads; 0 or 1 sets the switch (bit 1 of the usb-fast switch mask: 1 = the ring, the default; 0 = the old 8 slots,
 * all queued). The switch is read when the buffers are allocated (a block is sized by it) and at every queueing. Returns the switch in bit 0, bit 1 always set (so 0 means "no ring": the SDK's stub for an adapter without it), in bits 16..23 the slots of the live block and, in bits 8..15, the receive
 * window in frames of 64 KiB that the buffers allocated now can take: 0 when none are live or the window is the old one (the host keeps 2 DATA frames in flight then). */
i32 usb_bulk_ring(i32 set)
{
    u32 sr = sr_mask();
    u32 frames;
    i32 r;
    if (set >= 0)
        usb_bulk_ring_off = set != 0 ? 0u : 1u;
    frames = live ? rx_use() / RING_SLOTS_PER_FRAME : 0u;
    if (frames < 2u)
        frames = 0;
    r = (i32)((usb_bulk_ring_off ? 0u : 1u) | 2u | (frames << 8) | ((live ? rx_slots : 0u) << 16));
    sr_restore(sr);
    return r;
}

/* ---- the service imports of <dk/gen/imports.h> (docs/plans/sdk.md section 5.1) ----
   The new names of the calls above, with the handle that dk_usb_bulk_open gives out. The service has one owner (the I/O job of usb_bulk_enable), so the
   handle is only a token. */

static int usb_bulk_token;

int32_t dk_usb_bulk_open(dk_module_ctx *ctx, dk_usb_bulk **out)
{
    if (!ctx || !out)
        return -48; /* DK_E_CONTRACT */
    *out = (dk_usb_bulk *)&usb_bulk_token;
    return 0;
}

int32_t dk_usb_bulk_enable(dk_usb_bulk *ub, int32_t io_id)
{
    (void)ub;
    return usb_bulk_enable(io_id);
}

int32_t dk_usb_bulk_disable(dk_usb_bulk *ub, int32_t io_id)
{
    (void)ub;
    return usb_bulk_disable(io_id);
}

int32_t dk_usb_bulk_take(dk_usb_bulk *ub, void **buf, uint32_t *len)
{
    (void)ub;
    return usb_bulk_take((u8 **)buf, (u32 *)len);
}

int32_t dk_usb_bulk_release(dk_usb_bulk *ub, void *buf)
{
    (void)ub;
    return usb_bulk_release((u8 *)buf);
}

int32_t dk_usb_bulk_send(dk_usb_bulk *ub, const void *buf, uint32_t len)
{
    (void)ub;
    return usb_bulk_send((const u8 *)buf, len);
}

uint32_t dk_usb_bulk_state(dk_usb_bulk *ub)
{
    (void)ub;
    return usb_bulk_state();
}

int32_t dk_usb_bulk_takeover(dk_usb_bulk *ub)
{
    (void)ub;
    return usb_bulk_takeover();
}

int32_t dk_usb_bulk_giveback(dk_usb_bulk *ub)
{
    (void)ub;
    return usb_bulk_giveback();
}
