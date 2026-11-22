use super::*;

fn calm() -> Inputs {
    Inputs {
        record_unreadable: false,
        safe_once: false,
        safe_key_held: false,
        dsp_hang_seen: false,
        panic_seen: false,
        boot_counter: 0,
        package: PackageStatus::Ok,
    }
}

#[test]
fn nothing_wrong_is_normal() {
    assert_eq!(decide(&calm()), Level::Normal);
    assert_eq!(
        decide(&Inputs {
            boot_counter: SAFE_AFTER - 1,
            ..calm()
        }),
        Level::Normal
    );
}

#[test]
fn each_safe_trigger_gives_its_reason() {
    let cases = [
        (
            Inputs {
                record_unreadable: true,
                ..calm()
            },
            SafeReason::Unreadable,
        ),
        (
            Inputs {
                safe_once: true,
                ..calm()
            },
            SafeReason::Flag,
        ),
        (
            Inputs {
                safe_key_held: true,
                ..calm()
            },
            SafeReason::Key,
        ),
        (
            Inputs {
                dsp_hang_seen: true,
                ..calm()
            },
            SafeReason::DspHang,
        ),
        (
            Inputs {
                panic_seen: true,
                ..calm()
            },
            SafeReason::Panic,
        ),
        (
            Inputs {
                boot_counter: SAFE_AFTER,
                ..calm()
            },
            SafeReason::Counter,
        ),
    ];
    for (i, want) in cases {
        assert_eq!(decide(&i), Level::Safe(want));
    }
}

#[test]
fn safe_reason_codes_are_the_boot_info_reasons() {
    assert_eq!(SafeReason::Unreadable.code(), SAFE_READ);
    assert_eq!(SafeReason::Flag.code(), SAFE_FLAG);
    assert_eq!(SafeReason::Key.code(), SAFE_KEY);
    assert_eq!(SafeReason::DspHang.code(), SAFE_DSP);
    assert_eq!(SafeReason::Panic.code(), SAFE_PANIC);
    assert_eq!(SafeReason::Counter.code(), SAFE_COUNTER);
}

#[test]
fn the_first_safe_trigger_found_is_the_reason() {
    let all = Inputs {
        safe_once: true,
        safe_key_held: true,
        dsp_hang_seen: true,
        panic_seen: true,
        boot_counter: SAFE_AFTER,
        ..calm()
    };
    assert_eq!(decide(&all), Level::Safe(SafeReason::Flag));
    assert_eq!(
        decide(&Inputs {
            safe_once: false,
            ..all
        }),
        Level::Safe(SafeReason::Key)
    );
    assert_eq!(
        decide(&Inputs {
            safe_once: false,
            safe_key_held: false,
            ..all
        }),
        Level::Safe(SafeReason::DspHang)
    );
    assert_eq!(
        decide(&Inputs {
            safe_once: false,
            safe_key_held: false,
            dsp_hang_seen: false,
            ..all
        }),
        Level::Safe(SafeReason::Panic)
    );
    assert_eq!(
        decide(&Inputs {
            record_unreadable: true,
            ..all
        }),
        Level::Safe(SafeReason::Unreadable)
    );
}

#[test]
fn the_two_counter_thresholds() {
    assert!(SAFE_AFTER < CORE_ONLY_AFTER);
    assert_eq!(
        decide(&Inputs {
            boot_counter: CORE_ONLY_AFTER - 1,
            ..calm()
        }),
        Level::Safe(SafeReason::Counter)
    );
    assert_eq!(
        decide(&Inputs {
            boot_counter: CORE_ONLY_AFTER,
            ..calm()
        }),
        Level::CoreOnly(CoreOnlyReason::Crashes)
    );
    assert_eq!(
        decide(&Inputs {
            boot_counter: u32::MAX,
            ..calm()
        }),
        Level::CoreOnly(CoreOnlyReason::Crashes)
    );
}

#[test]
fn many_crashes_beat_a_safe_trigger() {
    let i = Inputs {
        safe_key_held: true,
        dsp_hang_seen: true,
        boot_counter: CORE_ONLY_AFTER,
        ..calm()
    };
    assert_eq!(decide(&i), Level::CoreOnly(CoreOnlyReason::Crashes));
}

#[test]
fn an_unreadable_record_has_no_counter_to_trust() {
    let i = Inputs {
        record_unreadable: true,
        boot_counter: CORE_ONLY_AFTER,
        ..calm()
    };
    assert_eq!(decide(&i), Level::Safe(SafeReason::Unreadable));
}

#[test]
fn a_package_that_cannot_run_is_core_only_and_not_a_crash() {
    for (status, want) in [
        (PackageStatus::Missing, CoreOnlyReason::PackageMissing),
        (
            PackageStatus::Incompatible,
            CoreOnlyReason::PackageIncompatible,
        ),
        (PackageStatus::Damaged, CoreOnlyReason::PackageDamaged),
        (
            PackageStatus::Unreadable,
            CoreOnlyReason::PackageUnreadable,
        ),
    ] {
        let level = decide(&Inputs {
            package: status,
            ..calm()
        });
        assert_eq!(level, Level::CoreOnly(want));
        assert!(!want.counts_as_crash());
        // it wins over every safe trigger: safe mode needs the package's menu
        let busy = Inputs {
            package: status,
            safe_key_held: true,
            safe_once: true,
            record_unreadable: true,
            panic_seen: true,
            ..calm()
        };
        assert_eq!(decide(&busy), Level::CoreOnly(want));
    }
    assert!(CoreOnlyReason::Crashes.counts_as_crash());
}

#[test]
fn a_package_fault_and_many_crashes_name_the_package() {
    let i = Inputs {
        package: PackageStatus::Damaged,
        boot_counter: CORE_ONLY_AFTER,
        ..calm()
    };
    assert_eq!(decide(&i), Level::CoreOnly(CoreOnlyReason::PackageDamaged));
}

#[test]
fn the_safe_key_does_not_hold_back_core_only_at_the_counter_threshold() {
    let i = Inputs {
        safe_key_held: true,
        boot_counter: CORE_ONLY_AFTER,
        ..calm()
    };
    assert_eq!(decide(&i), Level::CoreOnly(CoreOnlyReason::Crashes));
}
