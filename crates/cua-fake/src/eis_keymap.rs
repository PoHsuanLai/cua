//! The keymap the fake EIS keyboard sends: a few US keys, written out in full so no xkb data
//! file is read. Evdev codes: a 30, s 31, space 57, Enter 28, Left Control 29, Left Shift 42.

/// The keymap text.
pub(crate) const US_KEYMAP: &str = r#"
xkb_keymap {
  xkb_keycodes "fake" {
    minimum = 8; maximum = 255;
    <RTRN> = 36; <LCTL> = 37; <AC01> = 38; <AC02> = 39; <LFSH> = 50; <SPCE> = 65;
  };
  xkb_types "fake" {
    virtual_modifiers NumLock;
    type "ONE_LEVEL" {
      modifiers = none; map[none] = Level1; level_name[Level1] = "Any";
    };
    type "ALPHABETIC" {
      modifiers = Shift + Lock; map[Shift] = Level2; map[Lock] = Level2;
      level_name[Level1] = "Base"; level_name[Level2] = "Caps";
    };
  };
  xkb_compat "fake" {
    interpret Any + AnyOf(all) { action = SetMods(modifiers = modMapMods, clearLocks); };
  };
  xkb_symbols "fake" {
    key <RTRN> { [ Return ] };
    key <LCTL> { [ Control_L ] };
    key <AC01> { [ a, A ] };
    key <AC02> { [ s, S ] };
    key <LFSH> { [ Shift_L ] };
    key <SPCE> { [ space ] };
    modifier_map Shift { <LFSH> };
    modifier_map Control { <LCTL> };
  };
};
"#;
