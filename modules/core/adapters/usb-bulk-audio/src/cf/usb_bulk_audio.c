/* core/usb-bulk-audio: a vendor bulk OUT interface (interface 5, EP2 OUT, 512-byte packets) added to the firmware's USB AUDIO/MIDI mode (port entry 5), with no mode switch and no
 * controller stop. Plan: docs/plans/usb-fast-audio-bulk.md. Facts: docs/findings/42-usb-transfer-routes.md section 4 (static, DT2 1.17 and DN2 1.12).
 *
 * The configuration of entry 5 has 5 interfaces (UAC2 audio, MIDI control, MIDIStreaming on EP1); EP2 IN is the audio clock feedback and EP2 OUT is unused. ua_prepare, called by a hook
 * at the entry of the firmware's USB init, copies the 0x148-byte descriptor to a static buffer, appends interface 5 and repoints the entry's connect and disconnect callbacks, descriptor
 * pointer and length. The receive ring is core/usb-bulk's (usb_bulk_port5_connect and usb_bulk_port5_disconnect): the wrappers here run the stock callback and call it.
 *
 * ua_prepare runs in the boot thread before the firmware's USB start, and again from the firmware tick (interrupt context) after a controller reset. It is idempotent, ISR-safe and
 * makes no runtime calls (the runtime may not exist yet). The connect and disconnect wrappers run in the USB interrupt. */

typedef unsigned int u32;
typedef unsigned char u8;

#define PORT_ENTRY_BYTES 0x28u
#define UA_ENTRY 5u
#define UA_STOCK_BYTES 0x148u
#define UA_BYTES 0x158u

/* firmware (anchor by hash in the port files) */
extern u8 usb_port_table[];
/* core/usb-bulk: the board's DDR window and the uncached alias, and the receive side of port 5 */
extern const u32 usb_alias_delta;
extern const u32 usb_ddr_base; /* the cached DDR window: base and size */
extern const u32 usb_ddr_bytes;
extern void usb_bulk_port5_attach(void);
extern void usb_bulk_port5_connect(void);
extern void usb_bulk_port5_disconnect(void);

/* usb_bulk_audio_state: 0 not prepared, 1 prepared, 2 refused (entry 5 is not the expected one, or the buffer lies outside the cached DDR window) */
volatile u32 usb_bulk_audio_state;
static u8 ua_desc[UA_BYTES + 8u]
    __attribute__((aligned(16))); /* 0x160: a whole number of 16-byte lines */
static u32 ua_stock_connect, ua_stock_disconnect;

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

/* the host's bus reset or SET_CONFIGURATION in USB AUDIO/MIDI: the stock work first, then EP2 OUT */
void ua_connect(void)
{
    u32 sr = sr_mask();
    ((void (*)(u32))ua_stock_connect)(
        1); /* the port 5 connect reads no argument; the slot is passed as for port 2, for safety */
    usb_bulk_port5_connect();
    sr_restore(sr);
}

/* the host is gone, the bus is reset or the mode switches: EP2 OUT first, then the stock callback */
void ua_disconnect(void)
{
    u32 sr = sr_mask();
    usb_bulk_port5_disconnect();
    sr_restore(sr);
    ((void (*)(void))ua_stock_disconnect)();
}

/* Make entry 5 ours (see above). ISR-safe and idempotent; never stops or restarts the controller. */
void ua_prepare(void)
{
    u32 *e = (u32 *)(usb_port_table + UA_ENTRY * PORT_ENTRY_BYTES);
    const u8 *cfg;
    u8 *d;
    u32 sr, i;
    if (usb_bulk_audio_state != 0u)
        return;
    sr = sr_mask();
    if (usb_bulk_audio_state != 0u) {
        sr_restore(sr);
        return;
    }
    cfg = (const u8 *)e[3];
    /* wTotalLength (bytes 2 and 3) is not checked: the firmware fills it in after this hook (FUN_4000677a, the first call of the USB init), so it is 0 here.
     * Our copy gets its own wTotalLength below. */
    if (e[4] != UA_STOCK_BYTES || cfg[0] != 9u || cfg[1] != 2u || cfg[4] != 5u ||
        (u32)ua_desc < usb_ddr_base ||
        (u32)ua_desc + sizeof ua_desc > usb_ddr_base + usb_ddr_bytes) {
        usb_bulk_audio_state = 2;
        sr_restore(sr);
        return;
    }
    /* push the bss lines of the descriptor out, then write through the alias only */
    for (i = 0; i < sizeof ua_desc; i += 16u)
        __asm__ volatile("cpushl %%bc,(%0)" : : "a"((u32)ua_desc + i) : "memory");
    d = alias_of(ua_desc);
    for (i = 0; i < UA_STOCK_BYTES; i++)
        d[i] = cfg[i];
    d[UA_STOCK_BYTES + 0] = 0x09; /* interface 5, alt 0, one endpoint, vendor class */
    d[UA_STOCK_BYTES + 1] = 0x04;
    d[UA_STOCK_BYTES + 2] = 0x05;
    d[UA_STOCK_BYTES + 3] = 0x00;
    d[UA_STOCK_BYTES + 4] = 0x01;
    d[UA_STOCK_BYTES + 5] = 0xff;
    d[UA_STOCK_BYTES + 6] = 0x00;
    d[UA_STOCK_BYTES + 7] = 0x00;
    d[UA_STOCK_BYTES + 8] = 0x00;
    d[UA_STOCK_BYTES + 9] = 0x07; /* EP2 OUT, bulk, 512, interval 0 */
    d[UA_STOCK_BYTES + 10] = 0x05;
    d[UA_STOCK_BYTES + 11] = 0x02;
    d[UA_STOCK_BYTES + 12] = 0x02;
    d[UA_STOCK_BYTES + 13] = 0x00;
    d[UA_STOCK_BYTES + 14] = 0x02;
    d[UA_STOCK_BYTES + 15] = 0x00;
    d[2] = UA_BYTES & 0xffu; /* wTotalLength, little endian */
    d[3] = UA_BYTES >> 8;
    d[4] = 6; /* bNumInterfaces */
    ua_stock_connect = e[0];
    ua_stock_disconnect = e[1];
    e[3] = (u32)d;
    e[4] = UA_BYTES;
    e[0] = (u32)ua_connect;
    e[1] = (u32)ua_disconnect;
    usb_bulk_port5_attach();
    usb_bulk_audio_state = 1;
    sr_restore(sr);
}
