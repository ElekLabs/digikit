//! `lfo` service, slots and hooks: none in this crate. The routines of the interrupt slots and the three UI-task hooks are assembler in the service file (`dks/src/hooks-*.s`); they call the
//! lookups of [`crate::records`] (`eval`, `name_into`, `wave_max`) and nothing else of this crate.
