# Conventions

cua follows the program's shared conventions, whose text lives in quire's `CONVENTIONS.md`
(identical in quire, sill, shell-host, palmrest and detent): types, traits, effects, errors,
tests, comments, change discipline, borrowing. Read it first. `ARCHITECTURE.md` here adds the
crate map, the one-home table, the traits, the recipes and the repo rules; where the two
disagree, `ARCHITECTURE.md` wins for cua. cua copies docket's and porter's repo shape and
inherits their additions, which hold here for the same reasons.

What cua adds or decides differently, each with its reason:

1. **Closed sets are enums without `Word`.** cua does not depend on quire's `ds-core`: the
   computer-use runtime must build for any desktop that offers the protocols. A closed set's
   stable slug is its serde `snake_case` form (the same slug on the bus, in the event log and in
   files); nothing hand-writes `slug` or `parse` (`cua_bus::slug_of` reads the serde form). Where
   a list of every variant is needed (`ControlRefusal::ALL`, `CuaMember::ALL`) it is written by
   hand and a test with a wildcard-free `match` stops the build when a variant is added.
2. **Async seams use `-> impl Future<Output = ...> + Send`**, so implementations write
   `async fn` and every future can cross a multi-threaded runtime. Closed sets of
   implementations are enums (`SeatBackend`, `CaptureBackend`) or generic parameters bundled in
   `cua_run::Seams`, never `dyn`.
3. **`todo!()` bodies exist only while an interface is frozen and its behaviour is not built.**
   Each one is listed in `FINDINGS.md` with the work that removes it. A test whose subject is a
   `todo!()` is `#[ignore = "<what closes it>"]` and carries real assertions.
4. **Pure crates reach no effect.** `cua-desktop`, `maskcap`, `seat-input`, `a11y-tree`,
   `cua-bus` and `cua-run` never reach `tokio`, `zbus`, `wayland-client`, `wayland-backend`,
   `reis`, `atspi`, an HTTP client, a database or the policy engine, whatever their
   dependencies' features; their backends are the C crates `maskcap-wayland`,
   `seat-ei` and `a11y-atspi`, which only `cua-system` and `cua-fake` use. `scripts/check-boundary.sh`
   fails the gate if a pure crate's tree reaches one.
5. **cua sits above docket and below sill.** It depends on stoker, porter, almanac and docket by
   sibling path; it depends on no sill crate and on nothing of the compositor, and the
   boundary script checks it. The compositor depends on `casement-protocols` alone.
6. **One message model.** A run's progress and its result are `prov::Message`s of kind
   `Report` (`Progress`, `Done`, `Failed`, `Cancelled`), sent to the requester through the router
   (`RunEffect::Report`, `cua_bus::report_draft`). cua defines no result or return type of its
   own: `CuaOutcome` is the detail the log keeps, and its `status()` picks the report's. **A
   message carries no authority**: what arrives for a run (`RunInput::Inbound`) is input, and
   nothing in it widens the lease, the mode, the budget or a permission; every action the run
   takes still goes through `Intents1.Gate.Check`.
7. **A model never gates and a screen never speaks.** Everything read off the screen (a node
   name, a title, an extracted value, a model's question) is labelled `Untrusted(Screen)` and
   shown as data. cua never decides policy: it names an action's effect (`classify_effect`) and
   asks the router. The model's thought and the frame's pixels are never stored; the log keeps
   digests.
8. **Step inputs and outputs are `Serialize + Eq`.** Every phase, input and effect of the run
   machine can be written to a trace and replayed; a pixel buffer's serde form is base64 and is
   for tests and traces only.
9. **Nothing ambient below the daemon.** The clock (`cua_run::Clock`), the desktop, the seat,
   capture, the accessibility bus, the model, the gate, the log and the message path are passed
   in (`Seams`); only `cuad` reads the system clock, the environment, a socket or the bus.
   Tests never touch the real system: no real seat, no real capture, no real D-Bus.
10. **Proposed values are `CuaConfig`.** Every number the specs mark "proposed" is a field of
    `cua_run::CuaConfig` with its default, and a key in `cua-host`'s table with a schema row; a test
    holds the schema to the defaults. The settings keys themselves belong to the person's settings.
11. **No `bool` in a field, a parameter or a message,** also in test doubles: a two-state enum
    says what it means (`Emulation::{Running, Stopped}`, `Press::{Down, Up}`).
12. **Typed points.** A point carries its coordinate space (`Point<WindowSpace>`); a bare pixel
    pair is wrong. The one place that names a global position is `seat-input::region`.
13. **One integration-test executable per crate.** Cargo links every file directly under `tests/`
    into its own executable, and each one statically links the crate's whole dependency graph
    (reis, wayland, zbus, tokio and the rest), so a build directory grows with the number of
    files. Integration tests are therefore modules of `tests/it/main.rs` (`mod <topic>;`),
    shared helpers are `tests/it/support/` modules (`use crate::support::...`), and goldens stay
    in `tests/golden/`. A separate target (`tests/<name>.rs` plus `[[test]]` in the crate's
    `Cargo.toml`, with a comment in `tests/it/main.rs`) needs a stated reason: it changes the
    environment (`set_var`, a panic hook, the current dir), holds a process-wide singleton the
    others must not share, needs its own `required-features`, or has `harness = false`. A new
    `tests/*.rs` without that reason is a mistake. Dependencies build without debug info
    (`[profile.dev.package."*"]` in `.cargo/config.toml`).
