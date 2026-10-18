//! Who is calling cuad: the process behind the connection, never a name it owns. Anyone can own
//! a well-known bus name (a program that starts while the shell restarts can take
//! `org.quire.Shell`), so a role comes from the process: porter's callers table (this daemon's
//! `cuad-callers.toml`, shipped as `dist/cuad-callers.toml`) names the unit or app scope behind
//! the sender, and a [`Standing`] says what a caller must be. Only the system's file can give a
//! fixed role; a scope is never more than a plain app.

use cua_bus::CuaCaller;
use docket_dbus::callers::{
    Caller, CallerFileError, CallerTable, Callers, INTENTD, Identity, Role, SHELL, Standing,
    read_table, table_from_toml,
};
use std::path::PathBuf;

/// The file name, under `/etc/quire` and the person's configuration directory.
pub const FILE: &str = "cuad-callers.toml";

const SHIPPED: &str = include_str!("../../../dist/cuad-callers.toml");

/// The bus name the shell is addressed by. It only addresses: whoever owns it is asked who it
/// is ([`SHELL`]) before it is told anything.
pub const SHELL_BUS: &str = "org.quire.Shell";

/// companiond, the planner: the `agent` role of the app `org.quire.Companion1`.
pub const COMPANION: Standing = Standing::new("org.quire.Companion1", Role::Agent);

/// The standing each caller class needs, in the order they are tried. The shell is docket's
/// [`SHELL`] and the router docket's [`INTENTD`], so cuad and the daemons beside it agree on who
/// they are.
const CLASSES: [(Standing, CuaCaller); 3] = [
    (INTENTD, CuaCaller::Router),
    (COMPANION, CuaCaller::Companion),
    (SHELL, CuaCaller::Shell),
];

/// The table the package ships. A shipped file that does not read is an empty table: nobody is
/// identified, which is the strict way to fail.
pub fn shipped() -> CallerTable {
    table_from_toml(SHIPPED).unwrap_or_default()
}

/// The table of this machine: `/etc/quire/cuad-callers.toml` with the person's file over it.
pub fn read(env: &impl Fn(&str) -> Option<String>) -> Result<CallerTable, CallerFileError> {
    #[cfg(feature = "fixture-callers")]
    if let Some(system) = env(SYSTEM_CALLERS_VAR) {
        return docket_dbus::callers::read_table_at(std::path::Path::new(&system), FILE, env);
    }
    read_table(FILE, env)
}

/// The test build's callers file in place of the system's (`fixture-callers` only).
#[cfg(feature = "fixture-callers")]
pub const SYSTEM_CALLERS_VAR: &str = "CUAD_TEST_SYSTEM_CALLERS";

/// The test build's process tree in place of `/proc` and the unit manager (`fixture-callers` only).
#[cfg(feature = "fixture-callers")]
pub const PROC_ROOT_VAR: &str = "CUAD_TEST_PROC_ROOT";

/// Who is calling over `connection`: the system's `/proc`, or in a test build the fixture tree
/// the harness names.
pub fn identity(
    connection: zbus::Connection,
    table: CallerTable,
    env: &impl Fn(&str) -> Option<String>,
) -> Identity {
    #[cfg(feature = "fixture-callers")]
    let fixture = env(PROC_ROOT_VAR).map(PathBuf::from);
    #[cfg(not(feature = "fixture-callers"))]
    let fixture: Option<PathBuf> = {
        let _ = env;
        None
    };
    Identity::on(connection, table, fixture.as_deref())
}

/// Which class `caller` is: the first whose standing it holds; `Other` when it holds none.
pub fn classify(caller: &Caller) -> CuaCaller {
    CLASSES
        .iter()
        .find(|(standing, _)| standing.held_by(caller))
        .map_or(CuaCaller::Other, |(_, class)| *class)
}

/// Who the connection `sender` (the message header's sender, a unique name) is. A message with
/// no sender, and a process nothing identifies, are `Other`.
pub async fn caller_of(who: &impl Callers, sender: Option<&str>) -> CuaCaller {
    let Some(sender) = sender else {
        return CuaCaller::Other;
    };
    who.caller_of(sender)
        .await
        .map_or(CuaCaller::Other, |caller| classify(&caller))
}

/// The unique name of the connection that owns [`SHELL_BUS`] if, and only if, its process holds
/// [`SHELL`]; `None` when nobody owns the name or the owner is not the shell. What only the shell
/// may hear is sent to this name and to no other.
pub async fn shell_owner(connection: &zbus::Connection, who: &impl Callers) -> Option<String> {
    let dbus = zbus::fdo::DBusProxy::new(connection).await.ok()?;
    let name = zbus::names::BusName::try_from(SHELL_BUS).ok()?;
    let owner = dbus.get_name_owner(name).await.ok()?;
    let standing = who.caller_of(owner.as_str()).await?;
    SHELL.held_by(&standing).then(|| owner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_core::{AppId, AppName, Isolation};

    fn caller(app: &str, role: Role) -> Caller {
        Caller {
            app: AppId {
                name: AppName::parse(app).expect("an app name"),
                isolation: Isolation::Unsandboxed,
            },
            role,
        }
    }

    #[test]
    fn a_caller_is_the_standing_its_process_holds() {
        let cases = [
            (
                "the router",
                caller("org.quire.Intents", Role::Agent),
                CuaCaller::Router,
            ),
            (
                "the planner",
                caller("org.quire.Companion1", Role::Agent),
                CuaCaller::Companion,
            ),
            (
                "the shell",
                caller("org.quire.Shell", Role::SheetHost),
                CuaCaller::Shell,
            ),
            (
                "an app scope called like the shell",
                caller("org.quire.Shell", Role::App),
                CuaCaller::Other,
            ),
            (
                "the router's name with the shell's role",
                caller("org.quire.Intents", Role::SheetHost),
                CuaCaller::Other,
            ),
            (
                "a stranger",
                caller("org.example.Evil", Role::App),
                CuaCaller::Other,
            ),
            (
                "a stranger with a fixed role",
                caller("org.example.Evil", Role::SheetHost),
                CuaCaller::Other,
            ),
        ];
        for (name, who, want) in cases {
            assert_eq!(classify(&who), want, "{name}");
        }
    }

    #[test]
    fn the_shipped_file_names_the_three_units_and_nothing_else() {
        let table = shipped();
        let rows = [
            ("intentd.service", "org.quire.Intents", Role::Agent),
            ("companiond.service", "org.quire.Companion1", Role::Agent),
            ("sill.service", "org.quire.Shell", Role::SheetHost),
        ];
        for (unit, app, role) in rows {
            let row = table.resolve_unit(unit).expect(unit);
            assert_eq!((row.app.name.as_str(), row.role), (app, role), "{unit}");
        }
        assert!(table.resolve_unit("sill-shell.scope").is_none());
    }
}
