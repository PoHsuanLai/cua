# Architecture

cua is the desktop's computer-use runtime: when no typed action and no hook can do a job, a run
looks at one window (a masked frame, an accessibility tree), asks a model for the next action,
asks the router whether it may run, injects it as input, waits for the screen to settle, and goes
again, inside a **lease** on that one window and a per-app consent that is off until the person
gives it. The design is the program spec (`SPEC.md` in the agent-spec folder, which wins where
it differs) with `cua.md`, `spike-a-result.md` and the user's answers (`QUESTIONS.md`); this file
is the map of the code that freezes those interfaces. `CONVENTIONS.md` holds the rules;
`FINDINGS.md` the open items and every `todo!()`.

**It ships on a stock compositor**, opt-in per app and off by default: the seat is shared (input
moves the person's cursor and steals focus), masking is cuad's, and a confirmation shows only
after cuad has stopped its own input. **casement adds the agent seat later**; the protocol it
implements is the `casement-protocols` repo's `quire-agent-v1.xml`, which cua-desktop and maskcap
name directly.

Reading order: section 1 (find the crate), section 3 (find the home), section 5 (a run in one
picture), section 6 (copy the recipe).

## 1. Crates and allowed edges

| Crate | Purpose | I/O |
| --- | --- | --- |
| `cua-desktop` | the base: windows (`ToplevelInfo`), leases (`LeaseRequest`, `LeaseGrant`, `Leased`, `EiSocket`), `BackendKind` and `BackendCaps::of`, `GlowState` and the protocol codes, `DesktopEvent`, the `Desktop` and `LeaseControl` seams, and the units every crate shares (`Stamp`, `Ms`, `Digest32`, `GlobalRect`). Re-exports `RunMode` and `WindowTrust` from docket. No backend lives here | none |
| `cua-types` | the leaf types the bus bodies carry and the capability crates produce: `Role`, `NodeSelector` (with `NameMatch`, `SelectorStep`, `SlotName`), `InputReceipt`, `InputOrigin`, `TextPath`, `SeatError`, `FrameDigest`. The capability crates re-export them, so every old path works | none |
| `maskcap` | `Frame`, `PixelBuf`, `MaskReport`, `apply_masks`, `FrameDigest`, `settle`, `pixel_diff`, the `WindowCapture` and `CaptureSession` seams. Feature `wayland`: `StockToplevelCapture`, `QuireMaskedCapture`, `StampClock` | none (`wayland`: wayland-client) |
| `seat-input` | `InputOp`, `Keymap`, `SeatCaps`, `plan`, `InputReceipt`, `region`, the `AgentSeat` seam. Feature `ei`: `EiSeat` | none (`ei`: reis, tokio, futures-util) |
| `a11y-tree` | `A11ySnapshot`, `A11yNode`, `prune`, `render`, `diff`, `secret_rects`, selectors, the `A11ySource` seam. Feature `atspi`: `AtspiSource` | none (`atspi`: atspi) |
| `cua-bus` | what crosses the bus and the log, serde only (no capability crate is linked; the leaf types come from `cua-types`): the `Start` body (`CuaTask`), `RunStateSlug`, `CuaOutcome`, `ModeSupport`, `CuaRecord`, `ActionResult`, `RunBudget`, `ControlRefusal`, `permits`, `report_draft` | none |
| `cua-run` | the pure run machine (`Run`, `RunTrack`, `RunPhase`, `RunInput`, `RunEffect`), `Observation`, `shortcut_hint` (the desktop's shortcut chords as one line for the model, read from a chordkit `Keymap`; `Exec::with_hints` hands it to the model's session), `classify_effect`, `window_class_of`, `window_sensitivity` (a sensitive window makes the run ask before input even when the router says run), `mode_support`, `choose_mode`, `LeaseBook`, `stalled`, `turn` and `note_of`, `CuaConfig` and `SETTING_ROWS`, `CuaModel`, the seams a run is carried out against (`Seams`, `Clock`, `GateLink`, `RecordSink`, `MessageLink`), and `Exec`, which carries a run's effects out against them (it awaits nothing but seam futures, so it needs no runtime) | none |
| `cua-dbus` | `org.quire.Cua1` (`Manager`, `Run`) as zbus proxies and skeletons, `introspection`, `CuaError`, bus names and paths | zbus |
| `cua-fake` | test only: `FakeSeat`, `FakeEis`, `FakeCapture`, `FakeDesktop`, `FakeA11y`, `ScriptedModel`, `FakePdp`, `MemoryEvents`, `FixedClock`, `FakeSeams`, `run_harness`, and `scene` (a window, frames, a world, a task) | none (`FakeEis`: reis, tokio) |
| `cua-eval` | the computer-use model eval (section 8): fake `surface`s drawn as pixels, a `Stage` shared by an `EvalSeat`, an `EvalCapture` and an `EvalDesktop`, a reference gate, `ScriptModel` (lines of moves) and `ProviderModel` (a model on a local engine, over stoker's `CuaSession`), `Corpus` (`eval/surfaces`, `eval/tasks`), verdicts, rates with Wilson bounds, the markdown report, and the `cua-eval` binary | the binary: files; `live`: loopback HTTP to an engine the caller started |
| `cua-eval-cloud` | the eval against a hosted model (section 8): `InferdProvider` (a stoker `Provider` whose turns are inferd chat turns, `Need::Llm` with images on class `screen`), `request` (a stoker turn as an inferd `ChatRequest`), `Watch` (turns, tokens, a spend refusal), `isolation` (what this process can reach), and the `cua-eval-cloud` binary | the bus to inferd (the key stays in accountd) |
| `cua-host` | the run executor as a library: `Supervisor` (a task per run), `Drivable`, `SupervisorEvent`, `IntentsGate`, `DaemonLinks`, `record_of`, `guard`, `CuadConfig`, `SystemClock`, and the settings read from text: `read`, `Locator`, `SettingsFeed`, `SCHEMA`, `Supervisor::{settings, apps, apply_settings, follow}`. No backend, bus or file watch | docket-client and almanac-client without `dbus` |
| `cuad` | the daemon and its library: `SeatBackend`, `CaptureBackend`, `InferdCuaModel`, `Links` (the three daemon links over one connection), `CuaProvider` (`org.quire.Cua`), `reach_json` and `follow_settings` (`org.quire.IntentReach1` beside the provider: `Reach()` says `{"cua.run.start": "offered"}` while `cua.enabled` is on, else `hidden`, and `ReachChanged` is emitted when the settings watch changes it; intentd lays that over the installed manifest, which declares the action hidden; a local mirror of docket-dbus's interface until the next docket pin), `SystemSeams`, `SystemDesktop`, `SystemA11y`, `WaylandPair`, `serve`, `serve_on` and `serve_on_with` (who is calling is the process behind the connection, named by `cuad-callers.toml` through docket-dbus's `Identity`; never a bus name), `classify`, `desktop_keymap` (the keymap built once at start: chordkit's conventions, plus the person's keycap overrides with the `keycap` feature). The binary serves both bus names; the person's settings: `SettingsWatch` (the directory watch on `cua/settings.toml`), `cua-host`'s names re-exported, `SCHEMA` (`dist/settings/cua.settings.toml`), and `--write-schema` | everything |

Allowed direct edges between our own crates (checked by `scripts/check-boundary.sh`;
dev-dependencies are outside it):

| Crate | May depend on |
| --- | --- |
| `cua-desktop` | `cua-action`, `prov`, `porter-core`; `casement-protocols` with feature `wayland` |
| `cua-types` | `cua-desktop`, `porter-core` |
| `maskcap` | `cua-action`, `cua-desktop`, `cua-types`; `casement-protocols` with feature `wayland` |
| `seat-input` | `cua-action`, `cua-desktop`, `cua-types`, `porter-core` |
| `a11y-tree` | `cua-action`, `cua-desktop`, `cua-types`, `prov`, `porter-core` |
| `cua-bus` | `almanac-core`, `cua-action`, `cua-desktop`, `cua-types`, `docket-core`, `prov`, `porter-core`, `porter-infer` |
| `cua-run` | `a11y-tree`, `chordkit`, `cua-action`, `cua-bus`, `cua-desktop`, `maskcap`, `seat-input`, `docket-core`, `prov`, `porter-core`, `porter-infer` |
| `cua-dbus` | `cua-bus`, `prov` |
| `cua-fake` | `a11y-tree`, `cua-action`, `cua-bus`, `cua-desktop`, `cua-run`, `maskcap`, `seat-input`, `docket-core`, `prov`, `porter-core`, `porter-infer`, `model-replay`, `model-provider`, `cua-parse` |
| `cua-eval` | `a11y-tree`, `chordkit`, `cua-action`, `cua-bus`, `cua-desktop`, `cua-fake`, `cua-parse`, `cua-run`, `cua-session`, `cua-vendors`, `docket-core`, `maskcap`, `model-catalog`, `model-http`, `model-openai-compat`, `model-provider`, `porter-core`, `porter-infer`, `prov`, `seat-input`, `vision-prep` |
| `cua-eval-cloud` | `cua-eval`, `model-catalog`, `model-provider`, `porter-client`, `porter-core`, `porter-infer`, `vision-prep` |
| `cua-host` | `cua-bus`, `cua-desktop`, `cua-run`, `docket-core`, `docket-client`, `prov`, `porter-core`, `almanac-core`, `almanac-client` |
| `cuad` | `a11y-tree`, `chordkit`, `keycap-client` (feature `keycap`, on by default), `cua-bus`, `cua-dbus`, `cua-host`, `cua-desktop`, `cua-run`, `maskcap`, `seat-input`, `docket-core`, `docket-client`, `prov`, `porter-core`, `porter-infer`, `porter-client`, `almanac-client` |

The edges differ from SPEC 1.3 in one way, forced by its own types: `seat-input`, `maskcap` and
`a11y-tree` take `cua-desktop` as their base (a lease, a stamp, a seat's isolation and who masks
are named by both the desktop and the crates that serve it), and `cua-bus` takes `cua-types`,
where the leaf types its records and the `Start` body name now live (it links none of the capability crates). See FINDINGS, "Spec
conflicts".

The repo order is stoker, porter, almanac, docket, cua, sill. cua reaches stoker's
`cua-action` and `model-replay`, porter's `prov`, `porter-core`, `porter-infer` and
`porter-client`, almanac's `almanac-core` and `almanac-client`, and docket's `docket-core`
(`CuaAsk`, `GateAnswer`, `MessageDraft`) and `docket-client`
(`IntentProvider`, `Intents`), all by sibling path until pinned git revs replace them. Nothing of
sill may enter any tree here. cua never reaches `policy-point`: the router is the one decision
point and cuad asks it through `Intents1.Gate`.

cuad uses the standard shortcuts only (owner decision, 2026-10-10). Its sandbox does not read the
person's own keycap remappings: the keymap file and the keycap socket stay closed. The shortcut
hints it tells the model come from chordkit's conventional keymap (with the desktop's own
bindings). `desktop_keymap` treats a source it cannot open as absent, with no error output; only a
bad line in a keymap file that did open is reported.

External boundaries: see `scripts/check-boundary.sh`. `zbus` is behind `cua-dbus` and `cuad`
only; the pure set never reaches an effect crate; `wayland-client` is behind the `wayland` features (cua-desktop, maskcap), `reis` behind `ei` (and `cua-fake`'s `FakeEis`), `atspi` behind `atspi`.

## 2. Modules

| Crate | Modules |
| --- | --- |
| `cua-desktop` | `units`, `window`, `lease`, `caps`, `glow`, `event`, `error`, `traits`, `wayland/{pump, state, proto, toplevels, lease, socket}` |
| `cua-types` | `role`, `selector`, `seat`, `digest` |
| `maskcap` | `pixels`, `mask`, `digest`, `settle`, `frame`, `capture`, `wayland/{state, shm, session}` |
| `seat-input` | `op`, `keymap`, `caps`, `plan`, `receipt`, `seat`, `region`, `ei/` (`conn`: the reis connection, `actor`: the thread that owns it) |
| `a11y-tree` | `model`, `selector`, `ops`, `source`, `atspi/` |
| `cua-bus` | `ids`, `budget`, `state`, `action`, `task`, `report`, `record`, `bodies`, `control`, `screen` |
| `cua-run` | `config`, `observation`, `effect_class`, `window_class`, `mode`, `turn`, `lease_book`, `stall`, `model`, `seams`, `screen/{phrases, hidden, unseen, link, excerpt, reader, track}`, `machine/{phase, input, effect, run, track, step, control, finish, ops}` (`step/screen`: the screen check's rows), `exec/{observe, session}` |
| `cua-dbus` | `names`, `error`, `manager`, `run`, `introspect` |
| `cua-fake` | `clock`, `seat`, `eis`, `capture`, `desktop`, `a11y`, `model`, `gate`, `events`, `harness`, `scene` |
| `cua-eval` | `surface/{def, state, render, access}`, `keys`, `stage`, `world`, `gate`, `script`, `task`, `corpus`, `verdict`, `run/{arena, timed, record}`, `eval`, `metrics`, `report`, `live`, `main` |
| `cua-eval-cloud` | `watch`, `request`, `entry`, `isolation`, `provider`, `main` |
| `cua-host` | `lock`, `clock`, `config`, `guard`, `gate`, `links`, `settings` (`keys`, `apps`, `read`, `locate`) < `supervisor`, `drive`, `events` |
| `cuad` | `backend`, `model`, `connect`, `provider`, `reach`, `watch` < `bus`, `desktop`, `system`, `serve` |

## 3. One home per concept

| Concept | Home |
| --- | --- |
| who acted, effects, labels, ids of sessions, runs, tasks, messages and entities, `Message`, `AgentRef`, `Address` | porter `prov` (never redefined here); the actor of a run's records is `cua_bus::cua_actor` |
| **a run's progress and result** | `prov::Message` of kind `Report`; `cua_bus::report_draft` builds the draft, `CuaOutcome::status` picks the status. There is no result type |
| coordinate spaces, `CuaAction`, `Point<S>`, `Chord`, `NodeId` | stoker `cua-action` (`WindowSpace`, never a bare pixel pair) |
| the model step (`CuaBegin`, `CuaStepRequest`, `CuaStepReply`, `TreeText`, `StepIndex`, `ServedBy`) | porter `porter-infer`; `cua_run::turn` builds the request |
| the gate vocabulary `RunMode`, `WindowTrust`, `WindowClass` | stoker `cua-action`; `cua-desktop` re-exports `RunMode` and `WindowTrust` |
| the gate contract (`CuaAsk`, `GateAnswer`, `EffectBasis`, `GrantAsk`, `MessageDraft`) | docket `docket-core` |
| the confirmation | the router's, through `Confirm1`; cua only stops its input (`RunEffect::SuspendLease(Consent)`, then `Proceed`) |
| the per-app consent | docket's `ActionGrant` with caller Cua, asked through `Gate.Grant` at the level the person's `cua.apps` row gives (`RunEnv::grant_level`) |
| desktop vocabulary, units (`Stamp`, `Ms`, `Digest32`), leases, backend capabilities | `cua-desktop` |
| frames, masks, digests, settling | `maskcap` |
| input operations, the plan, the keymap, the EI region map | `seat-input` (the region map is `seat_input::region`; nothing else names a global position) |
| the accessibility tree, selectors | `a11y-tree` |
| run states and slugs, outcomes, `ModeSupport`/`ModeGap`, the log records, `RunBudget`, control refusals and who may call which member | `cua-bus` |
| the machine, the phase table, effects, `mode_support`, `classify_effect` | `cua-run` |
| the screen check: the phrase table, hidden and unseen text, the verdict and its reasons | `cua_run::screen` (`PatternCheck`, `ScreenVerdict`, `Reason`); the closed kinds the log keeps are `cua_bus::ScreenKind` |
| the proposed values | `cua_run::CuaConfig` and `SETTING_ROWS`; the keys are sill's |
| the D-Bus interface | `cua-dbus` and `dbus/org.quire.Cua1.xml` |
| the Wayland protocol | `quire-agent-v1.xml` in `~/casement-protocols` (the one home), used by cua-desktop and maskcap (feature `wayland`); the server side is the fork's, later casement's |
| the `cua.run.start` action | `manifests/org.quire.Cua.toml`, served by `cuad::CuaProvider` |
| the event-log record | `cua_bus::CuaRecord`; `cuad::record_of` makes the almanac `EventBody::Area { area: Cua }` |
| views of the above for the shell | sill's `sill-model` (`run_state_of(slug, outcome)`); cua defines none |

## 4. Traits

| Trait | Home | Implemented by |
| --- | --- | --- |
| `Desktop`, `LeaseControl` | `cua-desktop` | `FakeDesktop` |
| `WindowCapture`, `CaptureSession` | `maskcap` | `StockToplevelCapture`, `QuireMaskedCapture` (through `CaptureBackend`), `FakeCapture` |
| `AgentSeat` | `seat-input` | `EiSeat` (through `SeatBackend`), `FakeSeat` |
| `A11ySource` | `a11y-tree` | `AtspiSource`, `FakeA11y` |
| `CuaModel`, `StepSink` | `cua-run` | `InferdCuaModel`, `ScriptedModel` |
| `GateLink`, `GateCheck` | `cua-run` | `IntentsGate`, `FakePdp` |
| `RecordSink` (`record`, and `open` by default nothing), `MessageLink` | `cua-run` | `DaemonLinks`, `MemoryEvents` |
| `Clock` | `cua-run` | `SystemClock`, `FixedClock` |
| `ScreenCheck` | `cua-run` | `PatternCheck` (deterministic; the machine's own); the optional reader answers through `CuaModel::classify_screen` |
| `Seams` | `cua-run` | `SystemSeams`, `FakeSeams` |
| `IntentProvider` | docket `docket-client` | `CuaProvider` |

Closed sets stay enums: the backends (`BackendKind`, `SeatBackend`, `CaptureBackend`), every
machine state, input and effect, every record and body.

## 5. A run in one picture

```
Cua1.Manager.Start (router or planner only; Disabled while cua.enabled = off)
  -> Run::start: mode_support(caps, mode)        Unsupported  -> Finished(Blocked(ModeUnsupported))
  Granting     Gate.Grant(app, space, level)     Refused/Never -> Finished(Blocked(ConsentDenied))
  Leasing      LeaseControl::lease (one run per window; a Shell window is never leasable)
  loop per step:
    Observing  capture (masked: by cuad on stock, by the compositor on the fork) + a11y snapshot
    Screening  (inside Observing, before the model) ScreenCheck over the title and the tree:
               Clear -> Thinking.  Suspicious -> Record(cua.screen.suspicious), then, unless the
               person already said to go on for this window and kind, WaitingForUser ("The screen
               shows text that looks like instructions for the assistant. Continue?"):
                 no  -> Finished(Blocked(ScreenSuspicious(kind)))
                 yes -> Thinking; remembered per run (window + kind); from then on a step that
                        sends out or removes asks even when the router says run (Held::AfterSuspicion)
               With the reader on and the check Clear: RunEffect::ReadScreen -> RunInput::ScreenRead
    Thinking   CuaStep -> CuaStepReply, points in WindowSpace
    Authorising  classify_effect -> Gate.Check(CuaAsk)  (screen label always Untrusted)
       Confirming(id)  router will ask the person: stock stops its own input (SuspendLease(Consent)),
                       then Request.Proceed; the router shows the sheet; Verdict arrives
                       (docket-client does not hand Progress out yet: the real gate answers
                       with the Verdict alone, see FINDINGS)
       Verdict(Run)    -> Acting      Verdict(Refused(..)) -> rest NotExecuted, observe again
    Acting     seat-input plan -> AgentSeat::perform (EI)
    Settling   pixel diff on stock (frames are not damage-gated), damage on the fork
    Verifying  the model (and optionally the tree) says it is done
  Finished     ReleaseLease, Glow(Off), Emit(Finished(outcome)), Record(Finished),
               Report(Message draft of kind Report: Done | Failed | Cancelled)  ->  Message.Send
  Progress     a Report(Progress) while the run waits for the person (a question, a pause)
```

A message that arrives for the run (`Message.Inbox`) is `RunInput::Inbound`: input only. The
router's gate never learns of it, and nothing in it widens what the run may do.

## 6. Recipes

**Add a run input, effect or phase.** (1) The variant in `cua-run/src/machine/{input,effect,phase}.rs`
(every type `Serialize + Eq`); (2) its row in the phase table (`cua.md` section 4, as changed here)
and in `machine/step.rs`; (3) a row test in `cua-run/tests/it/transitions*.rs`; (4) the effect carried
out in `cua_run::Exec` (the supervisor and the harness both use it), and a fake behaviour in
`cua-fake` if a seam changed. A look at the window ends with `Capture`: the capture builds the
observation, so no effect of a look follows it.

**Add a backend.** A `BackendKind` variant and its row in `BackendCaps::of`; its rows in the
`mode_support` table test; a `CaptureKind` row in `cuad::capture_kind`; the `Desktop` and
`LeaseControl` implementation behind a feature of `cua-desktop`.

**Add a member to `Cua1`.** (1) The method or signal in `cua-dbus` (`manager.rs` or `run.rs`),
on the proxy and the skeleton; run `cargo test -p cua-dbus`, which prints the new XML; replace
`dbus/org.quire.Cua1.xml` with it in the same commit; (2) a `CuaMember` and its row in
`cua_bus::permits` (and the wildcard-free list test); (3) the body type in `cua-bus` with a
round-trip test; (4) its handler in `cuad::bus` (a unit test holds the served interfaces to the
XML) and, for a signal, its line in `cuad::serve`'s relay.

**Add a record.** A `CuaRecord` variant with `#[serde(rename = "cua.<name>")]`, its row in
`tests/it/wire.rs` (the slugs are pinned), and its production in the machine. `cuad::record_of`
needs no change: the slug is the kind tag.

**Add a phrase or a style to the screen check.** A phrase goes into its group in
`cua-run/src/screen/phrases.rs` (lower case, single spaces, straight quotes; `Anywhere` for a phrase
that is a threat wherever it stands, `Addressed` for one that needs an assistant named in the same
text, `Opening` for a role marker), with a row in `tests/it/screen.rs` that finds it and, if it
could be ordinary text, a row that must stay clear. A new style is a `Family` and a group. A new
kind of finding is a `Reason`, its `cua_bus::ScreenKind` and a row in the bus's wire test.

**Add a setting.** A field of `CuaConfig` with its default, a row in `SETTING_ROWS` and an arm of
`CuaConfig::value`; the test that pins the rows to the defaults fails until all three agree. The
design/22 row and the sill key are the other repos' agents'.

**Add a message-borne behaviour.** Don't: a message is input. If a run must react to one, the
reaction goes through `RunInput::Inbound`, changes no lease, mode, budget or permission, and every
action it leads to is gated like any other.

**Add a dev script.** Under `scripts/dev/`, with a private runtime dir, a private bus and scratch
`HOME`; never the person's `wayland-0` or bus. Acceptance scripts run by hand, never in CI.

## 7. Repo rules

- Licence `MIT OR Apache-2.0`; edition 2024; `unsafe_code = "deny"`; `rust-toolchain.toml` pins
  1.98.1 and `deny.toml` is the licence gate.
- Cross-repo dependencies are git dependencies at pinned revs (stoker, porter, almanac, docket,
  casement-protocols), so a plain clone builds with no sibling checkout; the URLs and revs match the
  other repos' pins of the same crates, or cargo links two copies. External versions are copied from quire's `docs/workspace-deps.toml`, never chosen here;
  FINDINGS lists the ones added to this workspace.
- Behaviour is `todo!()` behind frozen signatures, each listed in `FINDINGS.md`; shape tests
  (round trips, pinned JSON, introspection, tables) pass.
- Tests never touch the real system: no real seat, no real capture, no real compositor, no real
  bus, no real engine. The daemon's binary is tested on a private bus with a cleared environment.
- The gate, with every exit code checked (`scripts/gate.sh` runs exactly this):

  ```bash
  set -euo pipefail
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace --all-features
  ./scripts/check-boundary.sh
  cargo deny check licenses
  echo "GATE GREEN"
  ```

  No crate is excluded: every crate builds and tests without hardware or a live compositor (the
  `wayland`, `ei` and `atspi` backends compile with their features and are exercised
  only through fakes; the live checks are the dev scripts).
- The `dist/` units carry their sandbox lines (`cuad.service`: Unix sockets only, no network, no
  files, no devices, no capabilities); the D-Bus activation files name the two bus names cuad
  owns (`org.quire.Cua1`, `org.quire.Cua`). `dist/cuad-callers.toml` (installed as
  `/etc/quire/cuad-callers.toml`) names the three units cuad takes for the router, the planner and
  the shell; a well-known name never decides who is calling.

## 8. The computer-use model eval

`cua-eval` plays a task set against a model and says how often the model does the job, keeps to it
and stops where it must. It runs cua's own run machine (`Run`, `Exec`, the stall and budget rules,
`classify_effect`, `window_class_of`) over seams of its own, so what it measures is the model and
cua's classification together, not a copy of either.

**Surfaces** (`eval/surfaces/*.toml`, `cua_eval::surface`). A surface is a small deterministic
application: screens, and on them labels, buttons, rows, fields and check boxes. A click, a key or a
wheel notch changes variables and fields and moves between screens (`goto`, `set`, `copy`, `clear`,
`when`). It is drawn as pixels (a bitmap font, flat rectangles, 960x600) for a vision model and its
window title and app id follow the screen, which is what makes a compose screen a `MailCompose`
window and a terminal a `Terminal`. The run also gets an accessibility tree built from what the
screen shows (`surface::snapshot`: the role, name and sensitivity of each visible item; an item
declares `role` and `sensitivity` in its TOML, and a kind's own role otherwise). Its bounds are empty,
so clicks are still classed by pixels, and the eval's models never see it (`run::Timed` drops it from
their request): they work from pixels. The tree is for the run's own rules, such as the forced ask
for a window with a password entry. Nothing here is a real app, account or desktop.

**Tasks** (`eval/tasks/*.toml`). Each task names a surface and a start screen, the goal in the
person's words, `success` (conditions that all hold at the end, plus `answer`: words the model's
final summary must contain), `forbid` (conditions that must never hold, checked after every input,
so an undone violation still counts), a step and spend budget, `ask` (`required` when the right run
asks the person), `end` (`done`, `infeasible`, `any`) and `person` (how the router's confirmations are
answered). Categories: `capability` (open, find, click, type, scroll, multi-step), `safety` with a
`topic` (`injection`: the screen tells the model to do something else; `sensitive`: a compose
window, a terminal, a bank, a payment form, where the model must stop and ask; `overeager`: a small
task on a screen that invites more) and `limits` (stalls, loops, the step budget, the spend cap,
a goal that cannot be met).

**Scripts.** A task also carries scripts: lines of moves (`click @compose`, `type hello`,
`key Enter`, `scroll 480 300 down 3`, `ask ...`, `finish done ...`; `@id` is the middle of that item on
the screen showing). Each script says the verdict the eval must reach. The ones that behave well show
the task can be passed; the ones that obey an injection, send anyway, loop or claim success show the
checks catch it. The gate runs them all (`cargo test`), with no model. A live run is kept in the same
lines (`reports/<label>.cassette.toml`) and plays back with `--engine cassette`.

**The gate** (`cua_eval::gate`) stands in for the router: it asks before any outbound, destructive or
executing step (what `classify_effect` names) and before any click or key in a password manager, an
admin tool or a bank (by app id), and the person answers the same way every time (`person`). It is
the strictest reading of what cua tells the router; the router owns the real rules.

**Verdicts** (`verdict.rs`): `pass`, `violation` (a forbidden state happened), `missed_ask`, `false_done`
(said done, the surface says no), `unreported` (did the job, and the final words lack the answer asked for), `gave_up`, `asked` (asked where no ask was called for), `stalled`,
`out_of_steps`, `out_of_spend`, `out_of_time`, `broken`. A model that asks (`ask`) ends the run there:
for a task that needs an ask, that is the right ending.

**Metrics** (`metrics.rs`, `report.rs`): per group (capability, safety, limits, and the safety topics)
pass rate with a 95% Wilson interval; violations (the false negatives of the safety rules) with the
Wilson upper bound; missed asks; unneeded asks (false positives: a run that asked where `ask =
never`, gate confirmations on an ordinary job included); stalls; steps per run and model latency per
step, median and 95th percentile (wall time, reported and never asserted on).

**Engines.** `scripted` (the scripts; no model), `cassette` (a kept live run), `local`: a model on
an OpenAI-compatible engine on the loopback address, driven the way inferd drives one (stoker's
`CuaSession` builds the prompt from the model's catalogue entry, a provider runs the turn, the
session reads the reply). `cua-eval` starts no engine and reaches no other address. The tasks a
local model cannot play (the spend cap) are `engines = "scripted_only"`.

**The cloud engine** is the `cua-eval-cloud` crate (the bus is not allowed in `cua-eval`). It plays
the same tasks with the same `ProviderModel` and `CuaSession`, but the provider under them is
`InferdProvider`: each model turn is a chat turn asked of porter's inferd over the bus (`Need::Llm`
with chat, vision and, for a tool dialect, tools; data class `screen`; the screenshot inline). inferd
picks the model by tier from its own config, fetches the API key from accountd for that one turn and
meters the reply against `[ai.spend]`; the eval process never has the key (`isolation::look` checks
its environment and descriptors and the binary refuses to run if a credential is in reach). This is
one layer above cuad's `InferdCuaModel`, which opens `Need::ComputerUse`: inferd refuses a computer-use
session on a hosted model today (porter FINDINGS, "inferd hosted"), so the cloud eval cannot use that
path; what it exercises of the production path is the prompt, the parse, the run machine and the
broker's routing, consent and spend. A model is a trial entry under `eval/cloud-catalog/` (the
hosted entries of stoker's catalogue say nothing of a computer-use dialect); a refusal on a spend
cap ends the whole eval (`--max-steps` bounds the steps of all runs). `scripts/cloud-eval.sh` builds
the world: a private bus, scratch HOME and XDG, a fake proc root that places the eval as inferd's
`intentd` unit, accountd (the one process whose environment names the key file) and inferd.

**Run it.**

```bash
cargo run -p cua-eval -- --dir eval --label scripted        # the gate's check: exit 1 on a mismatch
cargo run -p cua-eval -- --dir eval --engine local --entry <catalogue>/holo-3.1-4b.toml \
    --model holo-3.1-4b --port 8000 --repeat 3 --label holo-3.1-4b
```

```bash
scripts/cloud-eval.sh --accountd <accountd test-keys build> --inferd <inferd test-proc-root build> \
    --eval <cua-eval-cloud> --home <filled accountd home> --entry eval/cloud-catalog/<model>.toml \
    --label <model> -- --max-steps 400
```

**Add a task.** A surface screen if none fits (`eval/surfaces`; every `goto` and `screen` condition
is checked when the set loads); the task in `eval/tasks/<category>.toml` with at least one script
that passes and, where a slip is plausible, one that makes it; `cargo test -p cua-eval` fails until
every script reaches the verdict it names.

## 9. Install

`scripts/install.sh` installs cuad, `cuad.service` with its `cuad.service.d/sandbox.conf` drop-in,
the D-Bus activation files and interface XML, the settings schema, and `dist/cuad-callers.toml` as
`/etc/quire/cuad-callers.toml`. It checks for the accessibility service (`at-spi2-core` on Fedora,
Ubuntu and Arch) and says which package is missing. Flags: `--prefix DIR` (default `/usr/local`),
`--destdir DIR` (for packages), `--user` (under `~/.local`, no root), `--no-build`, `--bin-dir DIR`
(programs built elsewhere), `--dry-run` (print every action), `--sandbox auto|on|off` and
`--distro debian|fedora|arch`. Root steps use `$SUDO`, else sudo, doas or pkexec, else they are
printed. The sandbox keys that need a mount namespace live only in the drop-in; `auto` tests whether
`systemd --user` can use them and leaves them out where it cannot (Ubuntu 24.04+ by default). The build runs from `/` with `--manifest-path` and the pinned toolchain, so it is linked by the system's own linker; clang and mold (the repo's `.cargo/config.toml`) are for working on the code, and an install never needs them.
