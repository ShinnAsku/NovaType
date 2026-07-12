use crate::Key;

/// Windows virtual-key values used by the TSF shell.
pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21;
pub const VK_NEXT: u32 = 0x22;
pub const VK_OEM_MINUS: u32 = 0xBD;
pub const VK_OEM_PLUS: u32 = 0xBB;

/// Maps a Windows virtual key to the platform-independent session key.
#[must_use]
pub fn map_vk(vk: u32) -> Option<Key> {
    match vk {
        VK_BACK => Some(Key::Backspace),
        VK_ESCAPE => Some(Key::Escape),
        VK_SPACE => Some(Key::Space),
        VK_PRIOR | VK_OEM_MINUS => Some(Key::PagePrev),
        VK_NEXT | VK_TAB | VK_OEM_PLUS => Some(Key::PageNext),
        0x31..=0x39 => u8::try_from(vk - 0x30).ok().map(Key::Digit),
        0x41..=0x5A => char::from_u32(vk).map(|letter| Key::Char(letter.to_ascii_lowercase())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{VK_BACK, VK_ESCAPE, VK_OEM_MINUS, VK_SPACE, map_vk};
    use crate::Key;

    #[test]
    fn maps_letters_digits_and_controls() {
        assert_eq!(map_vk(0x4E), Some(Key::Char('n')));
        assert_eq!(map_vk(0x31), Some(Key::Digit(1)));
        assert_eq!(map_vk(VK_SPACE), Some(Key::Space));
        assert_eq!(map_vk(VK_BACK), Some(Key::Backspace));
        assert_eq!(map_vk(VK_ESCAPE), Some(Key::Escape));
        assert_eq!(map_vk(VK_OEM_MINUS), Some(Key::PagePrev));
    }

    #[test]
    fn ignores_unknown_keys() {
        assert_eq!(map_vk(0x70), None); // F1
    }
}
