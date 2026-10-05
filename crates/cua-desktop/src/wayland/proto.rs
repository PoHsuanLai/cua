//! `zcosmic_toplevel_info_v1`, generated from the vendored XML (`protocols/`): what stock
//! cosmic-comp says about a toplevel beyond `ext_foreign_toplevel_list_v1` (state bits and
//! geometry, and the ext-workspace each toplevel is on). The server side exists for the test
//! compositor only.

#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports)]
#![allow(missing_docs, missing_debug_implementations, clippy::all)]

pub mod client {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;
    use wayland_protocols::ext::workspace::v1::client::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
        use wayland_protocols::ext::workspace::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/cosmic-toplevel-info-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/cosmic-toplevel-info-v1.xml");
}
