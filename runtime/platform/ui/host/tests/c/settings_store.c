/* The fake of the settings imports (dk_settings_get, put and remove) for the host tests of the SDK components: a small key/value table. fs_put_result(rc) with
   rc < 0 refuses every put with rc; fs_reset clears the table and the counters. */
#include <stdint.h>
#include <string.h>
#include <dk/core/module.h>
#include <dk/core/status.h>

#define NKEYS 32
static struct {
    int used[NKEYS];
    char key[NKEYS][48];
    char val[NKEYS][256];
    uint32_t len[NKEYS];
    int put_result, puts, removes;
} st;

static int st_find(const char *key)
{
    int i;
    for (i = 0; i < NKEYS; i++)
        if (st.used[i] && !strcmp(st.key[i], key))
            return i;
    return -1;
}

int32_t dk_settings_get(dk_settings *s, const char *key, void *out, uint32_t cap)
{
    int i = st_find(key);
    (void)s;
    if (i < 0)
        return DK_E_SET_NOKEY;
    if (st.len[i] > cap)
        return DK_E_SET_VALUE;
    memcpy(out, st.val[i], st.len[i]);
    return (int32_t)st.len[i];
}

int32_t dk_settings_put(dk_settings *s, const char *key, const void *value, uint32_t len)
{
    int i = st_find(key);
    (void)s;
    st.puts++;
    if (st.put_result < 0)
        return st.put_result;
    if (i < 0)
        for (i = 0; st.used[i]; i++)
            ;
    st.used[i] = 1;
    strcpy(st.key[i], key);
    memcpy(st.val[i], value, len);
    st.len[i] = len;
    return 0;
}

int32_t dk_settings_remove(dk_settings *s, const char *key)
{
    int i = st_find(key);
    (void)s;
    st.removes++;
    if (i >= 0)
        st.used[i] = 0;
    return 0;
}

void fs_reset(void)
{
    memset(&st, 0, sizeof st);
}
void fs_put_result(int rc)
{
    st.put_result = rc;
}
int fs_puts(void)
{
    return st.puts;
}
int fs_removes(void)
{
    return st.removes;
}
