// The DSP part of a service fixture: dispatch code of the form that a hook stub of the image jumps to through a slot. It is not part of the manifest of test/svc-test
// (the provider has no DSP section there); the pack tests of patchkit add it with an `[[object]]` and a `[[fill]] space = "dsp"` and pack it for the DT2.
//
// It has one global text entry, one import of each kind that a dispatch routine has (a data word of the image and a code address of the image, both
// resolved against the import table of core/dyndsp at load) and no data of its own.

.SECTION/PM seg_pmco;
.GLOBAL _svctest_dsp_entry;
.EXTERN _svctest_dsp_word;
.EXTERN _svctest_dsp_back;

_svctest_dsp_nop:
    NOP;

_svctest_dsp_entry:
    R0 = DM(_svctest_dsp_word);
    R0 = R0 + 1;
    DM(_svctest_dsp_word) = R0;
    JUMP _svctest_dsp_back;
