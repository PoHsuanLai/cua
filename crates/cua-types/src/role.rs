//! What kind of thing an accessibility node is.

use serde::{Deserialize, Serialize};

/// What kind of thing a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Role {
    /// The window.
    Window,
    /// A dialog.
    Dialog,
    /// A push button.
    Button,
    /// A toggle button.
    ToggleButton,
    /// A check box.
    CheckBox,
    /// A radio button.
    RadioButton,
    /// A combo box.
    ComboBox,
    /// A menu bar.
    MenuBar,
    /// A menu.
    Menu,
    /// A menu item.
    MenuItem,
    /// A tab.
    Tab,
    /// A tab list.
    TabList,
    /// A link.
    Link,
    /// A text entry.
    Entry,
    /// A password entry: its value is never rendered and its region is masked.
    PasswordText,
    /// Static text.
    Text,
    /// A label.
    Label,
    /// A heading.
    Heading,
    /// A list.
    List,
    /// A list item.
    ListItem,
    /// A tree.
    Tree,
    /// A tree item.
    TreeItem,
    /// A table.
    Table,
    /// A table cell.
    Cell,
    /// A slider.
    Slider,
    /// A spin button.
    SpinButton,
    /// A scroll bar.
    ScrollBar,
    /// A progress bar.
    ProgressBar,
    /// An image.
    Image,
    /// A tool bar.
    ToolBar,
    /// A document.
    Document,
    /// A grouping section.
    Section,
    /// A role this list does not name, by its raw AT-SPI number.
    Other(u32),
}
