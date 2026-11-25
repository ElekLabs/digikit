/* test/local-patch-user: the module of test/local-ping-user (a hard need on one exported function, called once from init) for the service `local-patch`. A module of its own because the
 * service it imports is another module identity; the source is the one of local-ping-user, with the export renamed. */
#define local_ping_value local_patch_value
#include "../../../local-ping-user/src/c/user.c"
