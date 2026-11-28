/* Host test of src/cf/key_subscribers.c: the slots (first free, full, duplicate, unsubscribe and reuse) and the walk (slot order, the first consumer stops it, empty passes to the stock walker).
 * The walker shim (key-walk.s) and the entries of the info table are the patchkit rig test's (packages/patchkit/tests/key_subscribers.rs). */
#include <stdio.h>
#include <string.h>
#include "cf/key_subscribers.h"

static int failed;
#define CHECK(c)                                                                                   \
    do {                                                                                           \
        if (!(c)) {                                                                                \
            printf("%s:%d: check failed: %s\n", __FILE__, __LINE__, #c);                           \
            failed = 1;                                                                            \
        }                                                                                          \
    } while (0)

/* four subscribers that log their call in `order` and answer what `answer[i]` says */
static int order[16], calls;
static uint32_t answer[6];
static void *seen_vc, *seen_ev;

#define SUB(n)                                                                                     \
    static uint32_t sub##n(void *vc, const void *ev)                                               \
    {                                                                                              \
        order[calls++] = n;                                                                        \
        seen_vc = vc;                                                                              \
        seen_ev = (void *)ev;                                                                      \
        return answer[n];                                                                          \
    }
SUB(0)
SUB(1)
SUB(2)
SUB(3)
SUB(4)

static key_subscriber const subs[] = {sub0, sub1, sub2, sub3, sub4};

static void clear(void)
{
    for (unsigned i = 0; i < 5; i++)
        key_unsubscribe(subs[i]);
    memset(answer, 0, sizeof answer);
    calls = 0;
}

int main(void)
{
    int vc, ev;

    /* empty: the stock walker runs */
    CHECK(key_walk(&vc, &ev) == 0);

    /* slots in order; full; duplicate; null */
    CHECK(key_subscribe(sub0) == 0);
    CHECK(key_subscribe(sub1) == 1);
    CHECK(key_subscribe(sub2) == 2);
    CHECK(key_subscribe(sub3) == 3);
    CHECK(key_subscribe(sub4) == KEYS_FULL);
    CHECK(key_subscribe(sub2) == KEYS_DUPLICATE);
    CHECK(key_subscribe(0) == KEYS_NONE);

    /* none consumes: all run in slot order with the same arguments, the stock walker runs */
    CHECK(key_walk(&vc, &ev) == 0);
    CHECK(calls == 4 && order[0] == 0 && order[1] == 1 && order[2] == 2 && order[3] == 3);
    CHECK(seen_vc == &vc && seen_ev == &ev);

    /* the first consumer stops the walk */
    calls = 0;
    answer[1] = 1;
    answer[2] = 1;
    CHECK(key_walk(&vc, &ev) == 1);
    CHECK(calls == 2 && order[0] == 0 && order[1] == 1);

    /* a freed slot is reused first (slot order: the walk follows slots, not subscription order) */
    CHECK(key_unsubscribe(sub1) == KEYS_OK);
    CHECK(key_unsubscribe(sub1) == KEYS_NONE);
    CHECK(key_unsubscribe(0) == KEYS_NONE);
    CHECK(key_subscribe(sub4) == 1);
    calls = 0;
    memset(answer, 0, sizeof answer);
    answer[4] = 1;
    CHECK(key_walk(&vc, &ev) == 1);
    CHECK(calls == 2 && order[0] == 0 && order[1] == 4);

    /* every subscriber gone: empty again */
    clear();
    CHECK(key_walk(&vc, &ev) == 0 && calls == 0);
    CHECK(key_subscribe(sub3) == 0);

    clear();
    if (!failed)
        printf("ok\n");
    return failed;
}
