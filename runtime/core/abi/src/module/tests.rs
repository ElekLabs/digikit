use super::*;

#[test]
fn a_module_event_shows_the_module_name_when_the_header_was_read_else_the_leaf_without_dkm() {
    let id = |s: &[u8]| {
        let mut n = [0u8; LOG_NAME];
        n[..s.len()].copy_from_slice(s);
        n
    };
    // the header was read: the name part of the id, whatever the file is called
    assert_eq!(shown_name(b"digikit-usb-fast.dkm", &id(b"usb-fast")), id(b"usb-fast"));
    // no header (file missing, unreadable): the leaf without `.dkm`, cut to the log's width
    assert_eq!(shown_name(b"digikit-usb-fast.dkm", &[0; LOG_NAME]), id(b"digikit-usb-"));
    assert_eq!(shown_name(b"gone.dkm", &[0; LOG_NAME]), id(b"gone"));
    assert_eq!(shown_name(b"noext", &[0; LOG_NAME]), id(b"noext"));
}
