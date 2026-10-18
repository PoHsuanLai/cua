//! The selector and diff types survive serde, and the pinned JSON of a node is what other
//! crates read.

use a11y_tree::{
    A11yError, A11yTarget, NameMatch, NodeSelector, Resolution, Role, SelectorStep, SlotName,
    TreeDiff,
};
use cua_action::{Coord, NodeId, Size};
use cua_desktop::{Pid, WindowTitle};
use porter_core::AppName;

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: &T,
) -> String {
    let json = serde_json::to_string(value).unwrap();
    assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), value, "{json}");
    json
}

#[test]
fn selectors_resolutions_and_diffs_round_trip() {
    let selector = NodeSelector {
        path: vec![
            SelectorStep {
                role: Role::Link,
                name: NameMatch::Exact("Billing".into()),
            },
            SelectorStep {
                role: Role::Heading,
                name: NameMatch::Slot(SlotName("month".into())),
            },
            SelectorStep {
                role: Role::Other(116),
                name: NameMatch::Any,
            },
        ],
    };
    let json = round_trip(&selector);
    assert_eq!(
        json,
        r#"{"path":[{"role":{"kind":"link"},"name":{"kind":"exact","v":"Billing"}},{"role":{"kind":"heading"},"name":{"kind":"slot","v":"month"}},{"role":{"kind":"other","v":116},"name":{"kind":"any"}}]}"#
    );
    round_trip(&Resolution::One(NodeId(3)));
    round_trip(&Resolution::Many(vec![NodeId(1), NodeId(2)]));
    assert_eq!(round_trip(&Resolution::None), r#"{"kind":"none"}"#);
    round_trip(&TreeDiff {
        added: vec![NodeId(9)],
        removed: vec![NodeId(1)],
        changed: vec![],
    });
}

#[test]
fn a_target_and_every_error_round_trip() {
    let target = A11yTarget {
        pid: Pid(4242),
        app: AppName::parse("org.mozilla.firefox").unwrap(),
        title: WindowTitle("Sign in".into()),
        geometry: Size::new(Coord(1000), Coord(700)),
        space: prov::SpaceId::parse("desktop").unwrap(),
    };
    assert_eq!(
        round_trip(&target),
        r#"{"pid":4242,"app":"org.mozilla.firefox","title":"Sign in","geometry":{"w":1000,"h":700},"space":"desktop"}"#
    );
    for error in [
        A11yError::NoBus,
        A11yError::NotExposed,
        A11yError::NoMatchingWindow,
        A11yError::Timeout,
        A11yError::Gone,
        A11yError::Protocol("bad reply".into()),
        A11yError::BackendUnavailable,
    ] {
        round_trip(&error);
    }
    assert_eq!(round_trip(&A11yError::NoBus), r#"{"kind":"no_bus"}"#);
}

#[test]
fn a_node_pins_its_json_with_the_screen_label_on_its_name() {
    let node = &crate::support::gtk4_dialog().nodes[4];
    let json = serde_json::to_string(node).unwrap();
    assert!(
        json.starts_with(r#"{"id":4,"parent":{"kind":"node","v":0}"#),
        "{json}"
    );
    assert!(json.contains(r#""role":{"kind":"button"}"#), "{json}");
    assert!(
        json.contains(r#""name":{"value":"Save","label":{"integrity":"untrusted""#),
        "{json}"
    );
    assert!(json.contains(r#""sensitivity":"normal""#), "{json}");
}
