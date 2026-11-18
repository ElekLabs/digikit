// core/fx: the words of an effect row and the sizes the dispatch code of the fx service uses (the row layout is in core/fx's module.toml).
#ifndef FX_ROWS_INC
#define FX_ROWS_INC
#include "dk_fx.inc"

#define FX_PC 0
#define FX_DESC 1
#define FX_STATE 2
#define FX_EN 3
#define FX_P0_BYTES 16
#define FX_WAS 12
#define FX_MIX 13
#define FX_DIS 14
#define FX_WSTART 15
#define FX_WCOUNT 16
#define FX_NFAULT 17
#define FX_ROUTE 18
#define FX_AMT0_BYTES 76                  // word 19
#define FX_ROW_BYTES 96
#define FX_WINDOW 1500
#define FX_MAX_FAULTS 8
#endif
