use super::*;

fn wire() -> Wire {
    Wire {
        version: VERSION,
        request: 0,
        response: 0,
        command: 0,
        bank: 0,
        generation: 0,
        offset: 0,
        count: 0,
        result: 0,
        published: 0,
        playing: 0,
        published_generation: 0,
        frames: 0,
        pcm: [0; CHUNK_FRAMES],
        selected_track: 0,
        note_starts: 0,
    }
}

#[test]
fn live_and_published_banks_are_unavailable_until_the_peer_retires_the_voice() {
    let mut wire = wire();
    wire.published = 2;
    wire.playing = 1;
    let mailbox = Mailbox(&raw mut wire);
    assert!(matches!(mailbox.free_bank(), Err(E_BUSY)));

    unsafe { write_volatile(addr_of_mut!(wire.playing), 0) };
    let FreeBank(Bank(bank)) = mailbox.free_bank().unwrap();
    assert_eq!(bank, 1);
}

#[test]
fn write_waits_for_acknowledgment_and_preserves_signed_pcm() {
    let mut wire = wire();
    let mailbox = Mailbox(&raw mut wire);
    let bank = mailbox.free_bank().unwrap();
    mailbox.write(&bank, 3, 12, &[0x80, 0x00, 0x7f, 0xff]);
    assert_eq!(
        (
            wire.request,
            wire.bank,
            wire.generation,
            wire.offset,
            wire.count
        ),
        (1, 0, 3, 12, 2)
    );
    assert_eq!(&wire.pcm[..2], &[i16::MIN, i16::MAX]);
    assert_eq!(mailbox.acknowledged(), Ok(false));
    assert_eq!(mailbox.trigger(0, 256), Err(E_BUSY));

    unsafe { write_volatile(addr_of_mut!(wire.response), wire.request) };
    assert_eq!(mailbox.acknowledged(), Ok(true));
    unsafe { write_volatile(addr_of_mut!(wire.result), -2) };
    assert_eq!(mailbox.acknowledged(), Err(E_BUSY));
}

#[test]
fn a_stale_handle_cannot_start_the_replacement() {
    let mut wire = wire();
    wire.published = 2;
    wire.published_generation = 3;
    let mailbox = Mailbox(&raw mut wire);
    assert_eq!(mailbox.trigger(2, 256), Err(E_NOT_FOUND));
    assert_eq!(mailbox.trigger(3, 257), Err(E_FORMAT));
    assert_eq!(wire.request, 0);
    assert_eq!(mailbox.trigger(3, 256), Ok(()));
    assert_eq!((wire.request, wire.bank, wire.generation), (1, 1, 3));
}

#[test]
fn track_selection_is_bounded_and_manual_triggers_are_excluded() {
    let mut wire = wire();
    let mailbox = Mailbox(&raw mut wire);
    assert_eq!(mailbox.select_track(TRACKS + 1), Err(E_FORMAT));
    assert_eq!(wire.request, 0);
    assert_eq!(mailbox.requested_track(), 0);
    assert_eq!(mailbox.select_track(2), Ok(()));
    assert_eq!(mailbox.requested_track(), 2);
    assert_eq!(wire.selected_track, 0);
    assert_eq!(wire.command, Command::SelectTrack as u32);
    assert_eq!(wire.count, 2);
    assert_eq!(mailbox.select_track(0), Err(E_BUSY));
    unsafe {
        write_volatile(addr_of_mut!(wire.selected_track), 2);
        write_volatile(addr_of_mut!(wire.response), wire.request);
    }
    assert_eq!(mailbox.trigger(0, 256), Err(E_BUSY));
    assert_eq!(mailbox.requested_track(), 2);
    assert_eq!(mailbox.select_track(0), Ok(()));
    assert_eq!(mailbox.requested_track(), 0);
    assert_eq!(wire.selected_track, 2);
    assert_eq!(wire.count, 0);
}
