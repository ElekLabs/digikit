#[derive(Clone, Copy)]
pub(crate) struct Live {
    pub(crate) used: bool,
    pub(crate) dirty: bool,
    pub(crate) has_conf: bool,
    pub(crate) failed: bool,
    pub(crate) win: u8,
    pub(crate) hold: u8,
    pub(crate) word: u32,
    pub(crate) value: u32,
    pub(crate) ver: u16,
    pub(crate) conf_ver: u16,
    pub(crate) conf_value: u32,
    pub(crate) tries: u8,
}
pub(crate) const LIVE0: Live = Live {
    used: false,
    dirty: false,
    has_conf: false,
    failed: false,
    win: 0,
    hold: 0,
    word: 0,
    value: 0,
    ver: 0,
    conf_ver: 0,
    conf_value: 0,
    tries: 0,
};
