/* <dk/platform/data.h>: the files a module owns. A module's own files live under `data/<module id>/`, not next to the `.dkm`: the `modules/` list is
   capped at 32 entries. A module reads them with `dk_files_read` in `init`, or lists them with the `dk_files_*` calls (<dk/platform/files.h>, root
   DK_FILES_STORE). For now only the host writes them.

   Rules of dk_files_read here: one call at a time, and in `init` only (the read uses the runtime's staging buffer). The buffer you give it is
   yours: allocate it with dk_alloc and keep it until you no longer need the file. */
#ifndef DK_DATA_H
#define DK_DATA_H
#include <dk/core/module.h>

#define DK_DATA_E_PATH                                                                             \
    (-1) /* dk_data_path: the path does not fit, or `name` is empty or breaks the rules */

/* Writes "data/<module_id>/<name>" into `out` (`cap` bytes, NUL included) and returns its length, or DK_DATA_E_PATH when it does not fit, or
   when `name` is empty, holds '/' or holds "..". */
static inline int32_t dk_data_path(char *out, uint32_t cap, const char *module_id, const char *name)
{
    static const char prefix[] = "data/";
    uint32_t n = 0, i;
    if (!out || !module_id || !name || !name[0])
        return DK_DATA_E_PATH;
    for (i = 0; name[i]; i++) {
        if (name[i] == '/' || (name[i] == '.' && name[i + 1] == '.'))
            return DK_DATA_E_PATH;
    }
    for (i = 0; prefix[i]; i++) {
        if (n + 1 >= cap)
            return DK_DATA_E_PATH;
        out[n++] = prefix[i];
    }
    for (i = 0; module_id[i]; i++) {
        if (n + 1 >= cap)
            return DK_DATA_E_PATH;
        out[n++] = module_id[i];
    }
    if (n + 1 >= cap)
        return DK_DATA_E_PATH;
    out[n++] = '/';
    for (i = 0; name[i]; i++) {
        if (n + 1 >= cap)
            return DK_DATA_E_PATH;
        out[n++] = name[i];
    }
    out[n] = 0;
    return (int32_t)n;
}
#endif
