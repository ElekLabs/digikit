//! Which generated blocks a patch changed (route A of finding 55's emulator cost).
//!
//! `verify_code` hashes the loaded code per range. A range is a merged span of
//! many blocks, so one patched word used to bar every region with an entry in
//! the range. On a range whose hash differs the engine now decodes the loaded
//! words, compares them with the generated instruction table, and bars the
//! regions with an entry that could reach a differing PC. The merged range
//! bounds each block, including decoder lookahead. These functions hold that
//! logic without any generated data, so unit tests can run them.

use crate::rt::Insn;

/// A range with more differing PCs than this is barred as a whole (a range
/// not loaded yet differs everywhere, and a decode per PC is not worth it).
pub const MAX_CHANGED: usize = 512;

/// An instruction compared as the generated blocks see it: its type, kind,
/// length and field entries (`offset` is always 0).
#[derive(Clone, Copy, Debug)]
pub struct InsnKey(pub Insn);

impl PartialEq for InsnKey {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (&self.0, &other.0);
        a.type_name == b.type_name
            && a.kind == b.kind
            && a.length_bytes == b.length_bytes
            && a.fields.entries() == b.fields.entries()
    }
}

/// The PCs in `start .. start + len` where the generated table has an
/// instruction and `loaded` gives another one, or none (the loaded words do
/// not decode). PCs the table has no instruction at are skipped: no block
/// runs from them, and a change that reaches an instruction through the
/// decoder's lookahead shows at that instruction. None when more than `cap`
/// PCs differ.
pub fn changed_pcs<K: PartialEq>(
    start: u32,
    len: u32,
    generated: impl Fn(u32) -> Option<K>,
    mut loaded: impl FnMut(u32) -> Option<K>,
    cap: usize,
) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for pc in start..start + len {
        let Some(want) = generated(pc) else { continue };
        if loaded(pc).as_ref() != Some(&want) {
            if out.len() >= cap {
                return None;
            }
            out.push(pc);
        }
    }
    Some(out)
}

/// The entry PCs (sorted) that stay off after a code change. Each block starts
/// at its entry and can extend through other entries up to its hashed range's
/// exclusive end. Bar it if a PC in `changed` (sorted) falls in that span, or
/// if `range_end` returns None because its range is unknown or barred whole.
/// A region's bodies pass control to each other directly, so all its entries
/// go together.
pub fn barred_entries(
    region_entries: &[&[u32]],
    changed: &[u32],
    range_end: impl Fn(u32) -> Option<u32>,
) -> Vec<u32> {
    let hit = |pc: u32| {
        let Some(end) = range_end(pc) else {
            return true;
        };
        let first = changed.partition_point(|&c| c < pc);
        changed.get(first).is_some_and(|&c| c < end)
    };
    let mut barred = Vec::new();
    for entries in region_entries {
        if entries.iter().any(|&pc| hit(pc)) {
            barred.extend_from_slice(entries);
        }
    }
    barred.sort_unstable();
    barred.dedup();
    barred
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: usize = 130;

    fn insn(words: &[u16], pc: u32) -> Option<[u16; 3]> {
        let at = pc as usize;
        (at + 3 <= words.len()).then(|| [words[at], words[at + 1], words[at + 2]])
    }

    #[test]
    fn no_patch_bars_nothing() {
        assert!(barred_entries(&[&[10], &[30]], &[], |_| Some(60)).is_empty());
    }

    #[test]
    fn overlapping_blocks_are_bounded_by_their_merged_range() {
        // Blocks starting at 20 and 30 both hold PC 45. Only their regions
        // are barred, including a companion entry in a separate clean range.
        let entries: [&[u32]; 5] = [&[10], &[20, 100], &[30], &[60], &[110]];
        let range_end = |pc| match pc {
            10..16 => Some(16),
            20..80 => Some(80),
            100..120 => Some(120),
            _ => None,
        };
        assert_eq!(
            barred_entries(&entries, &[45], range_end),
            vec![20, 30, 100]
        );
        assert_eq!(
            barred_entries(&entries, &[30, 110], range_end),
            vec![20, 30, 100, 110]
        );
    }

    #[test]
    fn lookahead_can_change_an_instruction_before_the_patched_word() {
        let stock: Vec<u16> = (0..WORDS as u16).collect();
        let mut loaded = stock.clone();
        loaded[31] ^= 0x8000;
        let changed = changed_pcs(
            10,
            30,
            |pc| insn(&stock, pc),
            |pc| insn(&loaded, pc),
            MAX_CHANGED,
        )
        .unwrap();
        assert_eq!(changed, vec![29, 30, 31]);
        assert_eq!(
            barred_entries(&[&[10], &[30], &[32]], &changed, |_| Some(40)),
            vec![10, 30]
        );
    }

    #[test]
    fn a_range_that_cannot_be_narrowed_bars_its_regions() {
        let entries: [&[u32]; 3] = [&[10, 60], &[20], &[30]];
        let barred = barred_entries(&entries, &[], |pc| (pc >= 30).then_some(70));
        assert_eq!(barred, vec![10, 20, 60]);
    }

    #[test]
    fn too_many_changes_give_none() {
        let stock = |pc: u32| Some(pc);
        assert_eq!(changed_pcs(0, 10, stock, |_| None, 3), None);
        assert_eq!(
            changed_pcs(0, 10, stock, |_| None, 10).map(|v| v.len()),
            Some(10)
        );
        // A PC the table has nothing at is skipped, whatever is loaded.
        let sparse = |pc: u32| (pc % 2 == 0).then_some(pc);
        assert_eq!(
            changed_pcs(0, 10, sparse, |_| None, 10).map(|v| v.len()),
            Some(5)
        );
    }

    #[test]
    fn a_word_that_does_not_decode_counts_as_changed() {
        let stock: Vec<u16> = (0..WORDS as u16).collect();
        let changed = changed_pcs(
            0,
            10,
            |pc| insn(&stock, pc),
            |pc| insn(&stock[..5], pc),
            MAX_CHANGED,
        )
        .unwrap();
        assert_eq!(changed, vec![3, 4, 5, 6, 7, 8, 9]);
    }
}
