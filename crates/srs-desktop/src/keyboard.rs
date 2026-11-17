#![forbid(unsafe_code)]
//! Only unmodified foreground function keys are eligible. Text entry never binds.
use slint::winit_030::winit::keyboard::{Key, NamedKey};
const KEYS: [NamedKey; 12] = [
    NamedKey::F1,
    NamedKey::F2,
    NamedKey::F3,
    NamedKey::F4,
    NamedKey::F5,
    NamedKey::F6,
    NamedKey::F7,
    NamedKey::F8,
    NamedKey::F9,
    NamedKey::F10,
    NamedKey::F11,
    NamedKey::F12,
];
pub fn matches(key: &Key, binding: u8) -> bool {
    binding
        .checked_sub(1)
        .and_then(|index| KEYS.get(usize::from(index)))
        .is_some_and(|named| key == &Key::Named(*named))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bindings_are_bounded_and_mouse_only_captures_no_keyboard() {
        assert!(matches(&Key::Named(NamedKey::F8), 8));
        assert!(!matches(&Key::Named(NamedKey::F8), 0));
        assert!(!matches(&Key::Named(NamedKey::F8), 7));
        assert!(!matches(&Key::Character("a".into()), 8));
        assert!(!matches(&Key::Named(NamedKey::F8), 255));
    }
}
