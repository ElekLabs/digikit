// core/fx, the send hook stub and its shared data (fxsend.s on the Digitakt II, fxsend-dn2.s on the Digitone II): the part of both files that is the same.
// The stub reads the slot DK_SLOT_FX_SEND (see the header of the including file for the contract); the data are the send bus and the gains.

#include "dk_stub.inc"

.SECTION/PM seg_pmco;
.GLOBAL _fxs_entry;
.GLOBAL _fxs_idle;
.EXTERN _fxs_back;
.EXTERN _dl_slots;
.EXTERN _fx_s_r0;
.EXTERN _fx_sv_m13;
.EXTERN _fx_sv_i12;

_fxs_entry:
    DM(_fx_s_r0) = R0;
    R0 = DM(_dl_slots + DK_SLOT_FX_SEND);
    R0 = PASS R0;
    IF EQ JUMP _fxs_idle;
    DM(_fx_sv_m13) = M13;
    DM(_fx_sv_i12) = I12;
    M13 = 0;
    I12 = R0;
    JUMP (M13, I12);

_fxs_idle:
    R0 = DM(_fx_s_r0);
    JUMP _fxs_back;                       // the displaced instruction, then the master function

.SECTION/DM seg_dmda;
.PKDATA _fxs_bus, 260;                    // the pad word, then the send bus (32 left, 32 right floats)
.PKDATA _fxs_gnew, 64;                    // the 16 new gains
.PKDATA _fxs_prev, 256;                   // the old gains: 16 floats for each of up to 4 rows
