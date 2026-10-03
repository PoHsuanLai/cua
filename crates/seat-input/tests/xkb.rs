//! `Keymap::parse_xkb` over a keymap written out in full (no include files, so nothing of the
//! host's xkb data is read): characters at both levels, named keys, and what is left out.

use keyboard_types::Key;
use seat_input::{Keycode, Keymap, Produces, ShiftLevel};

const XKB: &str = r#"
xkb_keymap {
  xkb_keycodes "test" {
    minimum = 8; maximum = 255;
    <ESC> = 9; <AE01> = 10; <AE02> = 11; <BKSP> = 22; <TAB> = 23;
    <RTRN> = 36; <LCTL> = 37; <AC01> = 38; <AC02> = 39; <LFSH> = 50;
    <SPCE> = 65; <FK05> = 71; <KP1> = 87; <UP> = 111; <I219> = 219;
  };
  xkb_types "test" {
    virtual_modifiers NumLock;
    type "ONE_LEVEL" {
      modifiers = none; map[none] = Level1; level_name[Level1] = "Any";
    };
    type "TWO_LEVEL" {
      modifiers = Shift; map[Shift] = Level2;
      level_name[Level1] = "Base"; level_name[Level2] = "Shift";
    };
    type "ALPHABETIC" {
      modifiers = Shift + Lock; map[Shift] = Level2; map[Lock] = Level2;
      level_name[Level1] = "Base"; level_name[Level2] = "Caps";
    };
    type "KEYPAD" {
      modifiers = Shift + NumLock; map[Shift] = Level2; map[NumLock] = Level2;
      level_name[Level1] = "Base"; level_name[Level2] = "Number";
    };
  };
  xkb_compat "test" {
    interpret Any + AnyOf(all) { action = SetMods(modifiers = modMapMods, clearLocks); };
  };
  xkb_symbols "test" {
    key <ESC> { [ Escape ] };
    key <AE01> { [ 1, exclam ] };
    key <AE02> { [ 2, at ] };
    key <BKSP> { [ BackSpace ] };
    key <TAB> { [ Tab, ISO_Left_Tab ] };
    key <RTRN> { [ Return ] };
    key <LCTL> { [ Control_L ] };
    key <AC01> { [ a, A ] };
    key <AC02> { [ s, S ] };
    key <LFSH> { [ Shift_L ] };
    key <SPCE> { [ space ] };
    key <FK05> { [ F5 ] };
    key <KP1> { type = "KEYPAD", symbols[Group1] = [ KP_End, KP_1 ] };
    key <UP> { [ Up ] };
    key <I219> { [ dead_acute ] };
    modifier_map Shift { <LFSH> };
    modifier_map Control { <LCTL> };
  };
};
"#;

fn keymap() -> Keymap {
    Keymap::parse_xkb(XKB).expect("a keymap")
}

#[test]
fn characters_come_at_their_level_with_evdev_codes() {
    let keymap = keymap();
    let table = [
        ('a', 30, ShiftLevel::Plain),
        ('A', 30, ShiftLevel::Shifted),
        ('s', 31, ShiftLevel::Plain),
        ('1', 2, ShiftLevel::Plain),
        ('!', 2, ShiftLevel::Shifted),
        ('@', 3, ShiftLevel::Shifted),
        (' ', 57, ShiftLevel::Plain),
    ];
    for (c, code, level) in table {
        assert_eq!(keymap.stroke_for(c), Some((Keycode(code), level)), "{c:?}");
    }
    assert_eq!(keymap.stroke_for('z'), None, "a character the map lacks");
}

#[test]
fn named_keys_use_their_w3c_values() {
    let keymap = keymap();
    let table = [
        (Key::Enter, 28),
        (Key::Escape, 1),
        (Key::Backspace, 14),
        (Key::Tab, 15),
        (Key::Control, 29),
        (Key::Shift, 42),
        (Key::F5, 63),
        (Key::ArrowUp, 103),
    ];
    for (key, code) in table {
        assert_eq!(keymap.code_of(&key), Some(Keycode(code)), "{key:?}");
    }
    assert_eq!(
        keymap.code_of(&Key::Character("a".into())),
        Some(Keycode(30))
    );
    assert_eq!(
        keymap.code_of(&Key::Character("A".into())),
        Some(Keycode(30)),
        "a capital names the same key"
    );
}

#[test]
fn keypad_keys_and_dead_keys_produce_nothing() {
    let keymap = keymap();
    assert!(
        keymap.entries.iter().all(|e| e.code != Keycode(79)),
        "the keypad's second level is Num Lock, not Shift"
    );
    assert!(
        keymap.entries.iter().all(|e| e.code != Keycode(211)),
        "a dead key types nothing by itself"
    );
    assert_eq!(
        keymap.stroke_for('1'),
        Some((Keycode(2), ShiftLevel::Plain))
    );
}

#[test]
fn rows_follow_key_code_order_plain_before_shifted() {
    let keymap = keymap();
    let on_a: Vec<_> = keymap
        .entries
        .iter()
        .filter(|e| e.code == Keycode(30))
        .map(|e| (e.produces.clone(), e.level))
        .collect();
    assert_eq!(
        on_a,
        vec![
            (Produces::Char('a'), ShiftLevel::Plain),
            (Produces::Char('A'), ShiftLevel::Shifted)
        ]
    );
    let codes: Vec<u32> = keymap.entries.iter().map(|e| e.code.0).collect();
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted);
}

#[test]
fn text_that_is_not_a_keymap_is_malformed_not_a_panic() {
    for text in [
        "",
        "not a keymap",
        "xkb_keymap { xkb_keycodes \"x\" { <A> = ; }; };",
    ] {
        assert!(Keymap::parse_xkb(text).is_err(), "{text:?}");
    }
}
