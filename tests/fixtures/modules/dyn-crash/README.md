# Crashing module (test)

A module that raises a CPU exception a configured number of DSP blocks after its first tick: an illegal instruction (mode 0) or a division by zero (mode 1). The delay and the mode are the store file `cfg/crash-after.bin`. It tests the
digikit runtime's crash capture (docs/findings/46-digikit-debug.md): the firmware's exception handler calls the runtime's hook, which records the
fault in RAM that a soft reset keeps and resets; the next boot reports the record. Test fixture only.
