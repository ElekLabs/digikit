"""native_target: cargo target dirs keyed by the generated core."""

import tools.emulator.native_target as nt


def test_transpile_only_gen_is_generic_and_image_gen_is_hashed(tmp_path):
    plain = tmp_path / "plain"
    plain.mkdir()
    assert nt.gen_key(plain) == "generic"
    with_image = tmp_path / "img"
    with_image.mkdir()
    (with_image / "image.rs").write_text("")
    other = tmp_path / "img2"
    other.mkdir()
    (other / "image.rs").write_text("")
    key = nt.gen_key(with_image)
    assert key.startswith("g-") and len(key) == 10
    assert key == nt.gen_key(with_image)
    assert key != nt.gen_key(other)
