/* DN2 1.12 diagnostic only. Host publishes dd_mail.state last; staging is
 * immutable until DONE. ERROR is sticky until boot. No allocation or runtime API.
 * The stock sender consumes staging synchronously into its SPI queue. Staging
 * remains statically allocated even after an error; no timed retirement rule. */
typedef unsigned int u32;
typedef unsigned short u16;

#define DD_MAGIC 0xDDF4A601u
#define DD_ACK 0xA6DDu
#define DD_TIMEOUT 900u
#define DD_IDLE 0u
#define DD_BUSY 1u
#define DD_DONE 2u
#define DD_ERROR 3u
#define DD_START 4u

/* Mailbox words: state, err, stall, sent. Host only writes START at state. */
struct DdMailbox { volatile u32 state, err, stall, sent; };
struct DdMailbox dd_mail __attribute__((aligned(16)));
volatile u16 dd_frame[1344] __attribute__((aligned(16)));
static u32 dd_previous_quiet;
static u32 dd_failed;
extern void dd_stock_send(int len, const u16 *src, int reclen, const volatile u16 *rec);

static u32 frame_word(u32 i)
{
    return (u32)dd_frame[2u * i] | ((u32)dd_frame[2u * i + 1u] << 16);
}

static void fail(u32 err)
{
    dd_failed = 1;
    dd_mail.err = err;
    dd_mail.state = DD_ERROR;
}

void dd_frame_hook(int len, const u16 *src, int reclen, const volatile u16 *rec)
{
    u32 quiet = len == 0xa80 && reclen == 0xabc && src &&
        src[0] == 3 && src[1] == 0 &&
        (src[17] | src[18] | src[19] | src[20]) == 0;
    u32 safe = quiet && dd_previous_quiet;
    u32 state = dd_mail.state;
    dd_previous_quiet = quiet;
    if (dd_failed) {
        dd_mail.state = DD_ERROR;
    } else if (state == DD_START || state == DD_BUSY) {
        if (state == DD_START) {
            dd_mail.err = 0;
            dd_mail.stall = 0;
            dd_mail.sent = 0;
            dd_mail.state = DD_BUSY;
            if (frame_word(0) != 4 || frame_word(1) != DD_MAGIC ||
                frame_word(2) == 0 || frame_word(3) > 376 ||
                frame_word(6) > 640) {
                fail(101);
            }
        }
        if (!dd_failed && dd_mail.sent && rec && rec[0x60 / 2] == DD_ACK) {
            u32 ack = (u32)rec[0x60 / 2] | ((u32)rec[0x60 / 2 + 1] << 16);
            u32 token = (u32)rec[0x64 / 2] | ((u32)rec[0x64 / 2 + 1] << 16);
            if (token == frame_word(2) && ((ack >> 16) & 511u) == frame_word(3)) {
                u32 status = ack >> 25;
                if (status >= 2u) fail(status);
                else {
                    dd_mail.err = status; /* 1 = READY; 0 = accepted */
                    dd_mail.state = DD_DONE;
                }
            }
        }
        if (!dd_failed && dd_mail.state == DD_BUSY) {
            if (++dd_mail.stall > DD_TIMEOUT) fail(100);
            else if (safe) {
                ++dd_mail.sent;
                dd_stock_send(0xa80, (const u16 *)dd_frame, reclen, rec);
                return;
            }
        }
    }
    dd_stock_send(len, src, reclen, rec);
}
