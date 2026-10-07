//! `zcosmic_toplevel_info_v1`, server side, generated from the vendored XML.
#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports)]
#![allow(missing_docs, missing_debug_implementations, clippy::all)]

use wayland_protocols::ext::foreign_toplevel_list::v1::server::*;
use wayland_protocols::ext::workspace::v1::server::*;
use wayland_server;
use wayland_server::protocol::*;

pub mod __interfaces {
    use wayland_protocols::ext::foreign_toplevel_list::v1::server::__interfaces::*;
    use wayland_protocols::ext::workspace::v1::server::__interfaces::*;
    use wayland_server::protocol::__interfaces::*;
    wayland_scanner::generate_interfaces!("protocols/cosmic-toplevel-info-v1.xml");
}
use self::__interfaces::*;

wayland_scanner::generate_server_code!("protocols/cosmic-toplevel-info-v1.xml");
