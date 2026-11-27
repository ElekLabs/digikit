use crate::common::*;

/// RAM words that decide whether MainScreenView starts the filesystem check: mounted flag, superblock state, checked word.
/// The check starts only when mounted != 0 and (state == 3 or checked == 0); a successful check leaves state 4 and checked 1.
pub(crate) const FS_STATE_DONE: u32 = 4;

/// Resolves the three words from the predicate that starts the check (`mounted && (state == 3 || checked == 0)`): a function that calls
/// the state getter, compares with 3, then calls the checked getter. Each getter tests the mounted flag and loads its word.
/// Returns `(mounted, state, checked)`.
pub(crate) fn resolve_fs_check_flags(image: &[u8]) -> Result<(u32, u32, u32), String> {
    let sig = hex("4eb9000000007203b280670e4eb9000000004a8057c044804e7570014e75");
    let mut mask = vec![false; sig.len()];
    mask[2..6].fill(true);
    mask[14..18].fill(true);
    let predicate = unique(image, &sig, &mask).map_err(|e| format!("fs check predicate: {e}"))?;
    let word = |addr: u32, len: usize| -> Result<&[u8], String> {
        let off = addr.checked_sub(MAIN_LOAD).ok_or("fs check below MAIN")? as usize;
        image
            .get(off..off + len)
            .ok_or("fs check outside MAIN".to_owned())
    };
    let be = |bytes: &[u8]| u32::from_be_bytes(bytes.try_into().expect("four bytes"));
    let head = word(predicate, 18)?;
    let (state_getter, checked_getter) = (be(&head[2..6]), be(&head[14..18]));
    // getter: ... tst.l (mounted).l at +0x16 (4ab9), move.l (word).l,D2 at +0x2c (2439)
    let getter = |addr: u32| -> Result<(u32, u32), String> {
        let body = word(addr, 0x32)?;
        if body[0x16..0x18] != [0x4a, 0xb9] || body[0x2c..0x2e] != [0x24, 0x39] {
            return Err(format!(
                "fs flag getter {addr:#010x} has an unexpected shape"
            ));
        }
        Ok((be(&body[0x18..0x1c]), be(&body[0x2e..0x32])))
    };
    let (mounted, state) = getter(state_getter)?;
    let (mounted_again, checked) = getter(checked_getter)?;
    let in_ram = |a: u32| (0x4000_0000..0x4800_0000).contains(&a) && a & 3 == 0;
    if mounted != mounted_again
        || ![mounted, state, checked].into_iter().all(in_ram)
        || state == checked
    {
        return Err("fs check flag operands invalid".into());
    }
    Ok((mounted, state, checked))
}
