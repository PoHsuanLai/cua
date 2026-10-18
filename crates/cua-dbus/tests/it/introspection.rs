//! The checked-in introspection file is the interface the skeletons declare. A change to a
//! signature changes the file in the same commit.

use cua_bus::CuaMember;
use cua_dbus::{FILE_NAME, introspection};
use std::path::PathBuf;

fn file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../dbus")
        .join(FILE_NAME)
}

#[test]
fn introspection_matches_xml() {
    let expected =
        std::fs::read_to_string(file()).unwrap_or_else(|e| panic!("{}: {e}", file().display()));
    let actual = introspection();
    assert!(
        actual == expected,
        "dbus/{FILE_NAME} differs from the interfaces; it should read:\n{actual}"
    );
}

#[test]
fn every_member_of_the_spec_is_declared() {
    let xml = introspection();
    let want = [
        "interface name=\"org.quire.Cua1.Manager\"",
        "<method name=\"Start\">",
        "<method name=\"Runs\">",
        "<method name=\"StopAll\">",
        "<property name=\"Backend\" type=\"s\" access=\"read\"/>",
        "<property name=\"Modes\" type=\"a{ss}\" access=\"read\"/>",
        "<property name=\"SightNote\" type=\"s\" access=\"read\"/>",
        "<signal name=\"RunAdded\">",
        "<signal name=\"RunRemoved\">",
        "interface name=\"org.quire.Cua1.Run\"",
        "<method name=\"Cancel\">",
        "<method name=\"Pause\">",
        "<method name=\"Resume\">",
        "<method name=\"Answer\">",
        "<method name=\"TakeOver\">",
        "<method name=\"HandBack\">",
        "<property name=\"Id\" type=\"s\" access=\"read\"/>",
        "<property name=\"State\" type=\"s\" access=\"read\"/>",
        "<property name=\"Step\" type=\"u\" access=\"read\"/>",
        "<property name=\"Goal\" type=\"s\" access=\"read\"/>",
        "<property name=\"Window\" type=\"s\" access=\"read\"/>",
        "<property name=\"Mode\" type=\"s\" access=\"read\"/>",
        "<property name=\"Budget\" type=\"s\" access=\"read\"/>",
        "<signal name=\"StateChanged\">",
        "<signal name=\"StepStarted\">",
        "<signal name=\"ThoughtDelta\">",
        "<signal name=\"ActionProposed\">",
        "<signal name=\"ActionDone\">",
        "<signal name=\"AskUser\">",
        "<signal name=\"Finished\">",
    ];
    for w in want {
        assert!(xml.contains(w), "missing {w}");
    }
    // One method per member the allow-list table names (cua-bus): nothing is callable that the
    // table does not rule on.
    assert_eq!(xml.matches("<method ").count(), CuaMember::ALL.len());
}

#[test]
fn there_is_no_result_member_because_a_result_is_a_report_message() {
    let xml = introspection();
    assert!(
        !xml.contains("Result"),
        "a run's result is a prov::Message Report, not a bus member"
    );
    assert!(xml.contains("<signal name=\"Finished\">"));
}
