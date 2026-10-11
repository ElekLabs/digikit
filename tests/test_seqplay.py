"""tools/seqplay.py: note events from the frames sent to the DSP."""

import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'tools'))

import seqplay  # noqa: E402


def capture(icount, on=0, off=0, notes=()):
    frame = bytearray(48)
    for voice, note in enumerate(notes):
        struct.pack_into('>H', frame, 2 + 2 * voice, int(note * 256))
    struct.pack_into('>4H', frame, 34, on, off, on, off)
    return {'icount': icount, 'hex': frame.hex()}


def test_a_mask_held_over_two_frames_is_one_event():
    caps = [capture(0), capture(10, on=1), capture(20, on=1), capture(30, off=1), capture(40)]
    found = seqplay.events(caps)
    assert [(e.icount, e.kind, e.voices) for e in found] == [
        (10, 'on', (0,)), (30, 'off', (0,))]


def test_the_same_voice_twice_in_a_row_needs_a_gap_frame():
    caps = [capture(0, on=2), capture(5), capture(9, on=2)]
    assert [e.icount for e in seqplay.events(caps)] == [0, 9]


def test_unison_sets_several_voice_bits():
    assert seqplay.events([capture(1, on=0x8003)])[0].voices == (0, 1, 15)


def test_gaps_and_tempo():
    caps = []
    for k in range(4):
        caps += [capture(k * 66_000_000, on=1 << k), capture(k * 66_000_000 + 1000)]
    gaps = seqplay.note_on_gaps(seqplay.events(caps))
    assert gaps == [66_000_000] * 3
    assert round(seqplay.bpm(gaps[0] / 4), 1) == 120.0


def test_step_counts_wrap_to_the_first_trig():
    assert seqplay.step_counts([1, 5, 9, 13], 5) == [4, 4, 4, 4, 4]
    assert seqplay.step_counts([1, 4], 3) == [3, 13, 3]


def test_short_captures_are_skipped():
    assert seqplay.events([{'icount': 0, 'hex': '00' * 10}]) == []


def test_the_note_is_read_from_each_voice_word_at_the_note_on():
    caps = [capture(0, on=0b101, notes=(60, 0, 64.5)),
            capture(9, off=0b101, notes=(60, 0, 64.5))]
    found = seqplay.events(caps)
    assert found[0].notes == (60.0, 64.5)
    assert found[1].notes == ()


def test_a_few_instructions_of_jitter_is_inside_the_tolerance():
    base = [(0, 'on', (0,), (60.0,)), (16_544_000, 'off', (0,), ())]
    mine = [(0, 'on', (0,), (60.0,)), (16_544_018, 'off', (0,), ())]
    assert seqplay.first_difference(base, mine, 1000) is None
    assert seqplay.first_difference(base, mine, 0) is not None


def test_a_different_voice_note_or_count_is_a_difference_at_any_tolerance():
    base = [(0, 'on', (0,), (60.0,)), (5, 'off', (0,), ())]
    assert seqplay.first_difference(base, [(0, 'on', (1,), (60.0,)), base[1]], 10**9)
    assert seqplay.first_difference(base, [(0, 'on', (0,), (61.0,)), base[1]], 10**9)
    assert seqplay.first_difference(base, base[:1], 10**9)


def test_signature_counts_from_the_first_note_on():
    found = [seqplay.NoteEvent(100, 'off', (0,)), seqplay.NoteEvent(500, 'on', (0,), (60.0,)),
             seqplay.NoteEvent(900, 'off', (0,))]
    assert seqplay.signature(found) == [(0, 'on', (0,), (60.0,)), (400, 'off', (0,), ())]


def test_the_script_selects_each_track_with_trk_held():
    steps = seqplay.script({1: [1], 2: [3]}, '10M')
    assert steps.index('press:16') < steps.index('tap:26')   # track 2 is trig key 26
    assert steps.count('press:16') == 2 and steps.count('release:16') == 2
    assert seqplay.script({1: [1]}, '10M').count('press:16') == 0


def test_turn_steps_are_packets_of_detents_for_each_unit():
    steps = seqplay.turn_steps(2)
    turns = [int(x.split(':')[2]) for x in steps if x.startswith('turn:')]
    assert sum(turns) == 2 * seqplay.DETENTS_PER_UNIT and max(turns) <= seqplay.PACKET
    assert sum(int(x.split(':')[2]) for x in seqplay.turn_steps(-1) if x.startswith('turn:'))         == -seqplay.DETENTS_PER_UNIT


def test_notes_are_edited_with_the_trig_held_inside_grid_recording():
    steps = seqplay.script({1: [1, 5]}, '10M', {1: 3})
    hold = steps.index('press:25')
    assert steps.index('tap:19') < hold < len(steps) - 1 - steps[::-1].index('tap:19')
    assert steps.index('release:25') > hold
    assert any(x.startswith('turn:1:') for x in steps[hold:steps.index('release:25')])


def test_the_trig_page_is_selected_only_when_notes_are_edited():
    assert 'tap:1' in seqplay.script({1: [1]}, '10M', {1: 2})
    assert 'tap:1' not in seqplay.script({1: [1]}, '10M')


def test_expected_octaves_climb_to_the_range_then_reset():
    assert seqplay.expected_octaves(7, 2) == [0, 12, 24, 0, 12, 24, 0]
    assert seqplay.expected_octaves(3, 0) == [0, 0, 0]


def test_runs_split_where_the_gap_is_longer_than_a_step():
    def mk(time):
        return seqplay.NoteEvent(time, 'on', (0,), (60.0,))
    groups = seqplay.runs([mk(0), mk(10), mk(20), mk(100), mk(110)], 10)
    assert [len(g) for g in groups] == [3, 2]


def test_the_arp_menu_is_opened_after_the_trigs_and_closed_before_play():
    steps = seqplay.script({1: [1]}, '10M', None, 8, (2, 1))
    assert steps.index('tap:51') > steps.index('tap:25')
    assert steps.index('tap:51') < steps.index('tap:20')
    menu = [y for y in steps[steps.index('tap:51'):steps.index('tap:20')] if y.startswith('turn:')]
    assert menu and menu[0].startswith('turn:1:')               # MODE is knob A
    assert any(y.startswith('turn:3:') for y in menu)           # RANGE is knob C
    assert any(y.startswith('turn:3:') for y in steps[:steps.index('tap:19')])   # LEN first


def test_a_chord_is_built_in_note_edit_with_the_trig_held_inside_grid_recording():
    steps = seqplay.script({1: [1, 5]}, '10M', chord=[7, -3], chord_len=4)
    hold = steps.index('press:25')
    assert steps.index('tap:19') < hold < steps.index('tap:53')            # NOTE EDIT key
    assert steps.index('release:25') > steps.index('tap:53')
    after = steps[steps.index('tap:53'):steps.index('tap:20')]
    assert after.count('tap:14') == 2                                       # FUNC + DOWN twice
    assert [y for y in after if y.startswith('turn:5:')][0] == 'turn:5:10'   # knob E: NOTE
    assert any(y.startswith('turn:5:-') for y in after)
    assert any(y.startswith('turn:7:') for y in after)                      # knob G: LEN
    assert after.index('tap:12') > max(i for i, y in enumerate(after) if y.startswith('turn:'))


def test_a_script_without_a_chord_never_opens_note_edit():
    assert 'tap:53' not in seqplay.script({1: [1]}, '10M', {1: 2}, 4, (2, 0))


def test_chord_notes_go_to_the_first_trig_of_track_one():
    steps = seqplay.script({1: [3, 9]}, '10M', chord=[4])
    assert 'press:27' in steps and 'press:25' not in steps                  # trig 3 is key 27


def test_the_arp_orders_of_a_chord_follow_the_manual():
    chord = [60.0, 67.0, 63.0]                       # as entered
    assert seqplay.expected_order(chord, seqplay.ARP_MODES['UP']) == [60.0, 63.0, 67.0]
    assert seqplay.expected_order(chord, seqplay.ARP_MODES['DOWN']) == [67.0, 63.0, 60.0]
    assert seqplay.expected_order(chord, seqplay.ARP_MODES['CYCL']) == [60.0, 63.0, 67.0, 63.0]
    assert seqplay.expected_order(chord, seqplay.ARP_MODES['TRUE']) is None
    assert seqplay.cycled([60.0, 63.0, 67.0, 63.0], 6) == [60.0, 63.0, 67.0, 63.0, 60.0, 63.0]


def test_the_arp_mode_read_back_is_the_setters_last_argument():
    def call(value):
        return {'pc': seqplay.SET_MODE, 'stack': ['0x40018ef4', '0x446cad50', f'{value:#010x}']}
    result = {'regs': [call(0), call(1), {'pc': '0x1', 'stack': ['0', '0', '0x9']}, call(3)]}
    assert seqplay.last_mode(result) == 3
    assert seqplay.last_mode({'regs': []}) is None


def test_a_pass_with_too_few_notes_is_refused_not_checked():
    def mk(time, note):
        return seqplay.NoteEvent(time, 'on', (0,), (note,))
    two = [mk(0, 60.0), mk(16, 63.0)]
    assert seqplay.chord_order_report(two, seqplay.ARP_MODES['UP'], 'UP', 3, 16) == 1
    three = [mk(0, 60.0), mk(16, 63.0), mk(32, 67.0)]
    assert seqplay.chord_order_report(three, seqplay.ARP_MODES['UP'], 'UP', 3, 16) == 0
    assert seqplay.chord_order_report(three, seqplay.ARP_MODES['DOWN'], 'DOWN', 3, 16) == 1
