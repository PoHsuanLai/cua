//! The fake surfaces: what a click, a key and the wheel do, and what they draw.

use crate::support::eval_dir;
use cua_action::ScrollDir;
use cua_eval::surface::{Cond, Hit, Stroke, Surface, SurfaceDef, render};
use std::sync::Arc;

fn def(name: &str) -> Arc<SurfaceDef> {
    let path = eval_dir().join("surfaces").join(format!("{name}.toml"));
    let text = std::fs::read_to_string(path).expect("a surface file");
    Arc::new(toml::from_str(&text).expect("a surface"))
}

fn files() -> Surface {
    Surface::new(def("files"), "list", &[]).unwrap()
}

#[test]
fn a_click_on_a_row_opens_its_detail_screen_and_the_title_follows() {
    let mut s = files();
    assert_eq!(s.title(), "Files - Documents");
    let [x, y] = s.centre_of("row-budget.csv").expect("on screen");
    assert_eq!(s.click(x, y), Hit::Item("row-budget.csv".into()));
    assert_eq!(s.screen_id(), "detail");
    assert_eq!(s.title(), "Files - budget.csv");
    assert!(s.holds(&Cond::Var(["sel".into(), "budget.csv".into()])));
}

#[test]
fn a_click_on_nothing_changes_nothing() {
    let mut s = files();
    assert_eq!(s.click(940, 590), Hit::Nothing);
    assert_eq!(s.screen_id(), "list");
}

#[test]
fn the_wheel_moves_the_pane_and_stops_at_its_ends() {
    let mut s = files();
    assert!(
        s.centre_of("row-zebra-notes.txt").is_none(),
        "below the fold"
    );
    s.scroll(ScrollDir::Down, 1);
    assert!(
        s.centre_of("row-zebra-notes.txt").is_none(),
        "one notch is not enough"
    );
    s.scroll(ScrollDir::Down, 10);
    assert!(s.centre_of("row-zebra-notes.txt").is_some());
    s.scroll(ScrollDir::Up, 50);
    assert!(s.centre_of("row-zebra-notes.txt").is_none());
    assert!(s.centre_of("row-report.txt").is_some());
}

#[test]
fn typing_goes_to_the_focused_field_and_enter_runs_its_acts() {
    let mut s = files();
    s.type_text("nothing has the focus on a list");
    let [x, y] = s.centre_of("row-report.txt").unwrap();
    s.click(x, y);
    let [x, y] = s.centre_of("rename").unwrap();
    s.click(x, y);
    assert_eq!(
        s.focused(),
        Some("newname"),
        "the rename screen focuses its field"
    );
    s.type_text("abc");
    s.press(Stroke::Backspace);
    assert!(s.holds(&Cond::Field(["newname".into(), "ab".into()])));
    s.press(Stroke::Enter);
    assert_eq!(s.screen_id(), "list");
    assert!(s.holds(&Cond::Var(["renamed_to".into(), "ab".into()])));
    assert!(s.holds(&Cond::Var(["renamed_from".into(), "report.txt".into()])));
}

#[test]
fn a_click_elsewhere_takes_the_focus_away_and_tab_gives_it_back() {
    let mut s = Surface::new(def("mail"), "compose", &[]).unwrap();
    assert_eq!(s.focused(), Some("to"));
    assert_eq!(s.click(2, 2), Hit::Nothing);
    assert_eq!(s.focused(), None);
    s.type_text("lost");
    s.press(Stroke::Tab);
    assert_eq!(s.focused(), Some("to"));
    s.press(Stroke::Tab);
    assert_eq!(s.focused(), Some("subject"));
    s.type_text("hi");
    assert!(s.holds(&Cond::Field(["subject".into(), "hi".into()])));
    assert!(s.holds(&Cond::Field(["to".into(), "".into()])));
}

#[test]
fn a_terminal_takes_typing_at_once() {
    let mut s = Surface::new(def("terminal"), "shell", &[]).unwrap();
    s.type_text("ls");
    s.press(Stroke::Enter);
    assert!(s.holds(&Cond::Var(["last".into(), "ls".into()])));
    assert!(s.holds(&Cond::Field(["cmd".into(), "".into()])));
}

#[test]
fn the_same_surface_draws_the_same_bytes_and_each_screen_differs() {
    let mut s = files();
    let first = render(&s);
    assert_eq!(first, render(&s));
    assert_eq!((first.size.w, first.size.h), (960, 600));
    let [x, y] = s.centre_of("row-report.txt").unwrap();
    s.click(x, y);
    assert_ne!(first.data, render(&s).data);
}

#[test]
fn a_check_toggles_and_the_text_shows_the_variables() {
    let mut s = Surface::new(def("settings"), "home", &[]).unwrap();
    let [x, y] = s.centre_of("display").unwrap();
    s.click(x, y);
    let [x, y] = s.centre_of("dark_mode").unwrap();
    s.click(x, y);
    assert!(s.holds(&Cond::Var(["dark_mode".into(), "on".into()])));
    s.click(x, y);
    assert!(s.holds(&Cond::Var(["dark_mode".into(), "off".into()])));
    assert_eq!(s.fill("a {dark_mode} b {nothing}c"), "a off b c");
}
