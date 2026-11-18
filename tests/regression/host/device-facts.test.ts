import { describe, expect, test } from 'vitest';
import { bootWindowFacts } from '@digi/test-support';
import { deviceProfile, portAnchorAddress } from '../runtime/device-data.ts';
import { FIRMWARE, selectDevices } from '../runtime/devices.ts';

// Device facts in devices/*.toml that name firmware addresses must agree with the resolved port of the image the tests run on.
// The struct offsets of the view stack and of the ConfirmWindow (`view_manager_off`, `list_off`, `list_prev_off`, `node_view_off`, `confirm_result_off`, `confirm_title_off`) stay facts of the file:
// the notes in devices/digitakt-ii.toml name the code that they were read from.
describe.each(selectDevices())('%s device facts against the port', (device) => {
  const anchor = (name: string) => portAnchorAddress(device, FIRMWARE[device], name);

  test('[ui] brain_ptr is the address of the cf.view.value.brain anchor', () => {
    expect(deviceProfile(device).ui.brain_ptr).toBe(anchor('cf.view.value.brain'));
  });

  test('every address of [ui.boot_window] is the address of its port anchor', () => {
    const facts = bootWindowFacts(device);
    if (!facts) return; // a device without a boot window has no such facts
    expect({
      main_vptr: facts.mainVptr,
      confirm_vptr: facts.confirmVptr,
      progress_vptr: facts.progressVptr,
      fs_mounted_ptr: facts.fsMountedPtr,
      fs_state_ptr: facts.fsStatePtr,
      fs_checked_ptr: facts.fsCheckedPtr,
    }).toEqual({
      main_vptr: anchor('cf.view.site.main_screen_vptr_lit'),
      confirm_vptr: anchor('cf.window.site.confirm_vptr_lit'),
      progress_vptr: anchor('cf.window.site.progress_vptr_lit'),
      fs_mounted_ptr: anchor('cf.storage.value.fs_mounted'),
      fs_state_ptr: anchor('cf.storage.value.fs_state'),
      fs_checked_ptr: anchor('cf.storage.value.fs_checked'),
    });
    expect(facts.brainPtr).toBe(anchor('cf.view.value.brain'));
  });
});
