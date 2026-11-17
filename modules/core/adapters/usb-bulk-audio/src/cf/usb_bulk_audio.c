/* core/usb-bulk-audio: a vendor bulk OUT interface (interface 5, EP2 OUT, 512-byte packets) added to the firmware's USB AUDIO/MIDI mode (port entry 5), with no mode switch and no
 * controller stop. Plan: docs/plans/usb-fast-audio-bulk.md. Facts: docs/findings/42-usb-transfer-routes.md section 4 (static, DT2 1.17 and DN2 1.12).
 *
 * The configuration of entry 5 has 5 interfaces (UAC2 audio, MIDI control, MIDIStreaming on EP1); EP2 IN is the audio clock feedback and EP2 OUT is unused. ua_prepare, called by a hook
 * at the entry of the firmware's USB init, copies the 0x148-byte descriptor to a static buffer, appends interface 5 and repoints the entry's connect and disconnect callbacks, descriptor
 * pointer and length. The receive ring is core/usb-bulk's (usb_bulk_port5_connect and usb_bulk_port5_disconnect): the wrappers here run the stock callback and call it.
 *
 * ua_prepare runs in the boot thread before the firmware's USB start, and again from the firmware tick (interrupt context) after a controller reset. It is idempotent, ISR-safe and
 * makes no runtime calls (the runtime may not exist yet). The connect and disconnect wrappers run in the USB interrupt. */

#include <stddef.h>
#include "dkrt.h" /* struct DkrtDevice */
#include "../../../usb-bulk/include/usb_bulk_port5.h"

typedef unsigned int u32;
typedef unsigned char u8;

#define PORT_ENTRY_BYTES 0x28u
#define UA_ENTRY 5u
#define UA_STOCK_BYTES 0x148u
#define UA_BYTES 0x158u

/* firmware (anchor by hash in the port files) */
extern u8 usb_port_table[];
/* the board, from the runtime's device object (struct DkrtDevice of dkrt.h, which only grows at its end): the uncached alias of DDR is this far above the cached
 * window, which is a base and a size */
_Static_assert(offsetof(DkrtDevice, ddr_bytes) == 28,
               "the DDR window words this file reads are where abi.toml puts them");
extern const DkrtDevice dkrt_dev;
#define usb_alias_delta (dkrt_dev.ddr_uncached_alias - dkrt_dev.ddr_cached_base)
#define usb_ddr_base (dkrt_dev.ddr_cached_base)
#define usb_ddr_bytes (dkrt_dev.ddr_bytes)

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

/* Windows binds WinUSB to interface 5 through three control requests (docs/plans/usb-winusb.md). the stub ua_setup_hook sits on the entry of the EP0 setup dispatcher and calls
 * ua_setup_handle. In USB AUDIO/MIDI with our interface attached it answers the three requests below through the firmware's EP0 send; the stub then returns to the USB interrupt.
 * Everything else runs the stock dispatcher from its first instruction. No runtime calls. */
/* bRequest of the vendor request that returns the descriptor set; free in the stock firmware */
#define UA_MSOS_VENDOR_CODE 0x44u
#define UA_MSOS_INDEX 7u /* wIndex of that request: MS_OS_20_DESCRIPTOR_INDEX */
#define UA_PORT_AUDIO 5u
#define UA_DEV_BYTES 18u
#define UA_BOS_BYTES 33u
#define UA_SET_BYTES 178u

/* firmware */
extern u8 usb_setup[];       /* the 8 setup bytes, wire order */
extern u8 usb_active_port[]; /* u32 in RAM */
extern void ep0_send(int len, const u8 *data);

/* an ASCII character as a UTF-16LE code unit */
#define U16(c) (c), 0x00

/* the descriptor tables keep one field a line, as in the spec tables */
/* clang-format off */
/* BOS: the platform capability that tells Windows to ask for the descriptor set with vendor code 0x44. Windows reads it once after enumeration. */
static const u8 ua_bos[] = {
    0x05, 0x0F, 0x21, 0x00, 0x01, /* BOS header: wTotalLength 0x21, one capability */
    0x1C, 0x10, 0x05, 0x00,       /* platform capability, 28 bytes */
    0xDF, 0x60, 0xDD, 0xD8, 0x89, 0x45, 0xC7, 0x4C, 0x9C, 0xD2, 0x65, 0x9D, 0x9E, 0x64, 0x8A, 0x9F, /* MS OS 2.0 UUID */
    0x00, 0x00, 0x03, 0x06,       /* dwWindowsVersion 8.1 */
    0xB2, 0x00,                   /* wMSOSDescriptorSetTotalLength */
    UA_MSOS_VENDOR_CODE, 0x00,    /* bMS_VendorCode, bAltEnumCode */
};

/* MS OS 2.0 descriptor set: function subset for interface 5 with compatible ID WINUSB and the interface GUID. Windows reads it with the vendor request after the BOS. */
static const u8 ua_set[] = {
    0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x06, 0xB2, 0x00, /* set header, wTotalLength 0xB2 */
    0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0xA8, 0x00,             /* configuration subset: value 0, length 0xA8 */
    0x08, 0x00, 0x02, 0x00, 0x05, 0x00, 0xA0, 0x00,             /* function subset: bFirstInterface 5, length 0xA0 */
    0x14, 0x00, 0x03, 0x00, 'W', 'I', 'N', 'U', 'S', 'B', 0x00, 0x00, /* compatible ID */
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,             /* sub-compatible ID */
    0x84, 0x00, 0x04, 0x00, 0x07, 0x00, 0x2A, 0x00,             /* registry property, REG_MULTI_SZ, name length 0x2A */
    U16('D'), U16('e'), U16('v'), U16('i'), U16('c'), U16('e'), U16('I'), U16('n'), U16('t'), U16('e'),
    U16('r'), U16('f'), U16('a'), U16('c'), U16('e'), U16('G'), U16('U'), U16('I'), U16('D'), U16('s'),
    0x00, 0x00,                                                 /* name NUL */
    0x50, 0x00,                                                 /* data length 0x50 */
    U16('{'), U16('7'), U16('F'), U16('4'), U16('5'), U16('7'), U16('3'), U16('9'), U16('F'), U16('-'),
    U16('0'), U16('5'), U16('6'), U16('3'), U16('-'), U16('4'), U16('5'), U16('C'), U16('1'), U16('-'),
    U16('8'), U16('2'), U16('3'), U16('A'), U16('-'), U16('4'), U16('3'), U16('A'), U16('1'), U16('4'),
    U16('A'), U16('D'), U16('7'), U16('F'), U16('7'), U16('4'), U16('5'), U16('}'),
    0x00, 0x00, 0x00, 0x00,                                     /* string NUL and list-ending NUL */
};

/* clang-format on */

/* the length fields above are the numbers 0xB2, 0xA8, 0xA0, 0x84, 0x2A, 0x50, 0x21 and 0x44; these checks tie them to the real sizes */
_Static_assert(sizeof ua_bos == UA_BOS_BYTES && UA_BOS_BYTES == 0x21, "BOS size");
_Static_assert(sizeof ua_set == UA_SET_BYTES && UA_SET_BYTES == 0xB2, "descriptor set size");
_Static_assert(UA_BOS_BYTES == 5 + 28, "BOS: header and platform capability");
_Static_assert(UA_SET_BYTES == 10 + 8 + 8 + 20 + 132,
               "set: header, subsets, compatible ID, property");
_Static_assert(0xA0 == 8 + 20 + 132 && 0xA8 == 8 + 0xA0 && UA_SET_BYTES == 10 + 0xA8,
               "subset lengths");
_Static_assert(132 == 0x84 && 0x84 == 2 + 2 + 2 + 2 + 0x2A + 2 + 0x50 && 0x2A == 20 * 2 + 2 &&
                   0x50 == 38 * 2 + 2 + 2,
               "property lengths");
_Static_assert(UA_MSOS_VENDOR_CODE == 0x44, "vendor code");

static u8 ua_dev[UA_DEV_BYTES];

/* wLength, clamped: the number of bytes to send */
static int ua_clamp(u32 have, u32 want)
{
    return (int)(want < have ? want : have);
}

/* Called by the stub at the entry of the setup dispatcher. Returns 1 if the request was answered, 0 if the stock dispatcher must run. */
int ua_setup_handle(void)
{
    const volatile u8 *s = (const volatile u8 *)usb_setup;
    u32 type = s[0], req = s[1], vlo = s[2], vhi = s[3];
    u32 index = (u32)s[4] | ((u32)s[5] << 8);
    u32 wlen = (u32)s[6] | ((u32)s[7] << 8);
    if (usb_bulk_audio_state == 1u && *(volatile u32 *)usb_active_port == UA_PORT_AUDIO) {
        if (type == 0x80u && req == 6u && vlo == 0u && vhi == 1u) {
            /* GET_DESCRIPTOR device: port 5's own descriptor with bcdUSB 2.01 (so the host asks for the BOS) and bcdDevice 1.01 (a fresh Windows cache key) */
            const u8 *src = (const u8 *)((u32 *)(usb_port_table + UA_ENTRY * PORT_ENTRY_BYTES))[2];
            u32 i;
            for (i = 0; i < UA_DEV_BYTES; i++)
                ua_dev[i] = src[i];
            ua_dev[2] = 0x01;
            ua_dev[3] = 0x02;
            ua_dev[12] = 0x01;
            ua_dev[13] = 0x01;
            ep0_send(ua_clamp(UA_DEV_BYTES, wlen), ua_dev);
            return 1;
        }
        if (type == 0x80u && req == 6u && vlo == 0u && vhi == 0x0Fu) {
            /* GET_DESCRIPTOR BOS: stock gives no answer */
            ep0_send(ua_clamp(UA_BOS_BYTES, wlen), ua_bos);
            return 1;
        }
        if (type == 0xC0u && req == UA_MSOS_VENDOR_CODE && index == UA_MSOS_INDEX) {
            /* vendor request: the MS OS 2.0 descriptor set */
            ep0_send(ua_clamp(UA_SET_BYTES, wlen), ua_set);
            return 1;
        }
    }
    return 0;
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
