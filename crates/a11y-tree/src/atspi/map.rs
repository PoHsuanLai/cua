//! AT-SPI words to the snapshot's: roles, state bits and action names.

use crate::model::{NodeAction, NodeState, Role};
use ::atspi::{State, StateSet};
use std::collections::BTreeSet;

/// The role a snapshot uses for an AT-SPI role.
pub(super) fn role(role: ::atspi::Role) -> Role {
    use ::atspi::Role as R;
    match role {
        R::Frame | R::Window => Role::Window,
        R::Dialog | R::Alert | R::FileChooser | R::ColorChooser | R::FontChooser => Role::Dialog,
        R::Button => Role::Button,
        R::ToggleButton => Role::ToggleButton,
        R::CheckBox | R::CheckMenuItem => Role::CheckBox,
        R::RadioButton | R::RadioMenuItem => Role::RadioButton,
        R::ComboBox => Role::ComboBox,
        R::MenuBar => Role::MenuBar,
        R::Menu | R::PopupMenu => Role::Menu,
        R::MenuItem | R::TearoffMenuItem => Role::MenuItem,
        R::PageTab => Role::Tab,
        R::PageTabList => Role::TabList,
        R::Link => Role::Link,
        R::Entry | R::Terminal | R::Autocomplete => Role::Entry,
        R::PasswordText => Role::PasswordText,
        R::Text | R::Static | R::Paragraph => Role::Text,
        R::Label | R::AcceleratorLabel | R::Caption => Role::Label,
        R::Heading => Role::Heading,
        R::List | R::ListBox => Role::List,
        R::ListItem => Role::ListItem,
        R::Tree | R::TreeTable => Role::Tree,
        R::TreeItem => Role::TreeItem,
        R::Table => Role::Table,
        R::TableCell | R::ColumnHeader | R::RowHeader | R::TableColumnHeader => Role::Cell,
        R::Slider => Role::Slider,
        R::SpinButton => Role::SpinButton,
        R::ScrollBar => Role::ScrollBar,
        R::ProgressBar | R::LevelBar => Role::ProgressBar,
        R::Image | R::Icon | R::Animation => Role::Image,
        R::ToolBar => Role::ToolBar,
        R::DocumentFrame
        | R::DocumentSpreadsheet
        | R::DocumentPresentation
        | R::DocumentText
        | R::DocumentWeb
        | R::DocumentEmail => Role::Document,
        R::Panel | R::Section | R::Grouping | R::Filler | R::Form => Role::Section,
        other => Role::Other(other as u32),
    }
}

/// The state bits a snapshot keeps.
pub(super) fn states(set: StateSet) -> BTreeSet<NodeState> {
    set.iter()
        .filter_map(|state| match state {
            State::Enabled | State::Sensitive => Some(NodeState::Enabled),
            State::Focusable => Some(NodeState::Focusable),
            State::Focused => Some(NodeState::Focused),
            State::Selected => Some(NodeState::Selected),
            State::Checked | State::Pressed => Some(NodeState::Checked),
            State::Expanded => Some(NodeState::Expanded),
            State::Editable => Some(NodeState::Editable),
            State::Visible => Some(NodeState::Visible),
            State::Showing => Some(NodeState::Showing),
            _ => None,
        })
        .collect()
}

/// The action an AT-SPI action name stands for.
pub(super) fn action(name: &str) -> NodeAction {
    match name.to_lowercase().as_str() {
        "click" => NodeAction::Click,
        "press" => NodeAction::Press,
        "activate" => NodeAction::Activate,
        "toggle" => NodeAction::Toggle,
        "expand" | "expand or contract" => NodeAction::Expand,
        _ => NodeAction::Other(name.to_owned()),
    }
}
