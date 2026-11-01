/* <dk/machine.h>: the machine (voice) contract for content modules.
   Hand-written. The layouts come from the generated <dk/gen/machine.h> and <dk/gen/desc.h> (from sdk/sdk.toml).

   Declare each machine in a C file of the module:

     #include <dk/machine.h>
     DK_MACHINE(pluck, .name = "Pluck", .short_name = "PLK", .render = "_pk_render",
                .note_on = "_pk_note_on", .desc = "_pk_desc");
     DK_PARAM(pluck, 0, pluck_decay, "Decay", "DECAY", 0, 127, 64);
     DK_PAGE(pluck, "PLUCK", "Pluck", DK_KNOB(0), DK_KNOB(1), DK_KNOB(2), DK_KNOB(3));

   A stereo machine (contract 1.2) adds `.stereo = 1`, sets DK_MACHINE_STEREO in the contract word of its dk_machine_desc, writes the right
   channel to in->out_r and returns DK_STEREO or-ed into the render status (docs/MODULE-AUTHORING.md 7.1).

   DK_PARAM values are knob units 0 .. 127, fractions allowed. The stored value is the unit times 256
   (0x7f00 = 127.0). The records go to the .dk.desc section. `patchkit` reads them; the device never sees them.
   Text longer than its field is a compile error. */
#ifndef DK_MACHINE_H
#define DK_MACHINE_H
#include <dk/gen/machine.h>
#include <dk/page.h>

/* A machine: ID is a C identifier and becomes the machine id. The rest are dk_desc_machine field initializers. */
#define DK_MACHINE(ID, ...)                                                      \
    DK_DESC_OVERRIDE_BEGIN                                                       \
    static const dk_desc_machine dk_desc_machine_##ID DK_DESC = {                \
        .kind = DK_DESC_MACHINE, .size = sizeof(dk_desc_machine), .id = #ID,     \
        __VA_ARGS__};                                                            \
    DK_DESC_OVERRIDE_END

/* Parameter word WORD of MACHINE. ID is a C identifier, unique in the module. */
#define DK_PARAM(MACHINE, WORD, ID, NAME, SHORT, MIN, MAX, DEF)                      \
    static const dk_desc_param dk_desc_param_##MACHINE##_##WORD DK_DESC = {          \
        .kind = DK_DESC_PARAM, .size = sizeof(dk_desc_param), .machine = #MACHINE,   \
        .id = #ID, .word = (WORD), .min = (int32_t)((MIN) * 256 + 0.5),              \
        .max = (int32_t)((MAX) * 256 + 0.5), .def = (int32_t)((DEF) * 256 + 0.5),    \
        .name = NAME, .short_name = SHORT}

#endif
