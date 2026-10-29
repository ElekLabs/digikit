/* DN2 1.12, emulator-only. Host verifies armed DSP DMA and page progress before
 * each request and publishes START last. The stock sender can wait forever;
 * the external runner must bound the emulator. No production timeout promise.
 * The host retains the complete request and numeric BE staging words until DONE.
 * No reply/ACK rows, ordinary frame sender, or sample API are touched. */
typedef unsigned int u32;
#define LP_START 1u
#define LP_BUSY 2u
#define LP_DONE 3u
#define LP_ERROR 4u
#define LP_PAGES 235u

struct LpMailbox { volatile u32 state, error, tag, sent; };
struct LpMailbox lp_mail __attribute__((aligned(16)));
volatile u32 lp_staging[1024] __attribute__((aligned(16)));
static u32 lp_failed, lp_next;
extern void lp_stock_send(u32 tag, const volatile u32 *source, u32 format);

void lp_tick(void)
{
    if (lp_failed) {
        lp_mail.state = LP_ERROR;
        return;
    }
    if (lp_mail.state != LP_START) return;
    if (lp_next >= LP_PAGES || lp_mail.tag != lp_next) {
        lp_failed = 1;
        lp_mail.error = 1;
        lp_mail.state = LP_ERROR;
        return;
    }
    lp_mail.state = LP_BUSY;
    /* format 1: numeric words in CF's BE memory are sent least byte first.
     * The request remains BUSY and staging stays retained until this returns. */
    lp_stock_send(lp_next, lp_staging, 1);
    ++lp_next;
    lp_mail.sent = lp_next;
    lp_mail.state = LP_DONE;
}
