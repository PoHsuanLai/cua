# Architecture

cua is the desktop's computer-use runtime: when no typed action and no hook can do a job, a run
looks at one window (a masked frame, an accessibility tree), asks a model for the next action,
asks the router whether it may run, injects it as input, waits for the screen to settle, and goes
again, inside a **lease** on that one window and a per-app consent that is off until the person
gives it. The design is the program spec (`SPEC.md` in the agent-spec folder, which wins where
it differs) with `cua.md`, `spike-a-result.md` and the user's answers (`QUESTIONS.md`); this file
is the map of the code that freezes those interfaces. `CONVENTIONS.md` holds the rules;
`FINDINGS.md` the open items and every `todo!()`.

**It ships on stock cosmic-comp**, opt-in per app and off by default: the seat is shared (input
moves the person's cursor and steals focus), masking is cuad's, and a confirmation shows only
after cuad has stopped its own input. **The fork adds the agent seat later** (`~/cosmic-comp`,
branch `quire`, GPL, not a dependency of anything here); the protocol it implements is this
repo's `quire-agent-protocol`.

Reading order: section 1 (find the crate), section 3 (find the home), section 5 (a run in one
picture), section 6 (copy the recipe).

## 1. Crates and allowed edges

| Crate | Purpose | I/O |
| --- | --- | --- |
| `cua-desktop` | the base: windows (`ToplevelInfo`), leases (`LeaseRequest`, `LeaseGrant`, `Leased`, `EiSocket`), `BackendKind` and `BackendCaps::of`, `GlowState` and the protocol codes, `DesktopEvent`, the `Desktop` and `LeaseControl` seams, and the units every crate shares (`Stamp`, `Ms`, `Digest32`, `GlobalRect`). Re-exports `RunMode` and `WindowTrust` from docket. Feature `cosmic`: `CosmicDesktop` | none (`cosmic`: wayland-client) |
| `maskcap` | `Frame`, `PixelBuf`, `MaskReport`, `apply_masks`, `FrameDigest`, `settle`, `pixel_diff`, the `WindowCapture` and `CaptureSession` seams. Feature `wayland`: `StockToplevelCapture`, `QuireMaskedCapture` | none (`wayland`: wayland-client) |
| `seat-input` | `InputOp`, `Keymap`, `SeatCaps`, `plan`, `InputReceipt`, `region`, the `AgentSeat` seam. Feature `ei`: `EiSeat` | none (`ei`: reis) |
| `a11y-tree` | `A11ySnapshot`, `A11yNode`, `prune`, `render`, `diff`, `secret_rects`, selectors, the `A11ySource` seam. Feature `atspi`: `AtspiSource` | none (`atspi`: atspi) |
| `quire-agent-protocol` | `protocols/quire-agent-v1.xml` and its wayland-scanner bindings; features `client` (cuad) and `server` (the fork) | none |
| `cua-bus` | what crosses the bus and the log, serde only: the `Start` body (`CuaTask`), `RunStateSlug`, `CuaOutcome`, `ModeSupport`, `CuaRecord`, `ActionResult`, `RunBudget`, `ControlRefusal`, `permits`, `report_draft` | none |
| `cua-run` | the pure run machine (`Run`, `RunPhase`, `RunInput`, `RunEffect`), `Observation`, `classify_effect`, `mode_support`, `choose_mode`, `LeaseBook`, `stalled`, `turn`, `CuaConfig` and `SETTING_ROWS`, `CuaModel`, and the seams a run is carried out against (`Seams`, `Clock`, `GateLink`, `RecordSink`, `MessageLink`) | none |
| `cua-dbus` | `org.quire.Cua1` (`Manager`, `Run`) as zbus proxies and skeletons, `introspection`, `CuaError`, bus names and paths | zbus |
| `cua-fake` | test only: `FakeSeat`, `FakeEis`, `FakeCapture`, `FakeDesktop`, `FakeA11y`, `ScriptedModel`, `FakePdp`, `MemoryEvents`, `FixedClock`, `FakeSeams`, `run_harness` | none (`FakeEis`: reis) |
| `cuad` | the daemon and its library: `SeatBackend`, `CaptureBackend`, `Supervisor`, `IntentsGate`, `InferdCuaModel`, `DaemonLinks`, `CuaProvider` (`org.quire.Cua`), `guard`, `CallerNames`, `CuadConfig`, `SystemSeams`, `serve`. The binary is a skeleton | everything |

Allowed direct edges between our own crates (checked by `scripts/check-boundary.sh`;
dev-dependencies are outside it):

| Crate | May depend on |
| --- | --- |
| `cua-desktop` | `cua-action`, `docket-core`, `prov`, `porter-core`; `quire-agent-protocol` with feature `cosmic` |
| `maskcap` | `cua-action`, `cua-desktop`; `quire-agent-protocol` with feature `wayland` |
| `seat-input` | `cua-action`, `cua-desktop`, `porter-core` |
| `a11y-tree` | `cua-action`, `cua-desktop`, `prov`, `porter-core` |
| `quire-agent-protocol` | nothing of ours |
| `cua-bus` | `a11y-tree`, `almanac-core`, `cua-action`, `cua-desktop`, `maskcap`, `seat-input`, `docket-core`, `prov`, `porter-core`, `porter-infer` |
| `cua-run` | `a11y-tree`, `cua-action`, `cua-bus`, `cua-desktop`, `maskcap`, `seat-input`, `docket-core`, `prov`, `porter-core`, `porter-infer` |
| `cua-dbus` | `cua-bus`, `prov` |
| `cua-fake` | `a11y-tree`, `cua-action`, `cua-bus`, `cua-desktop`, `cua-run`, `maskcap`, `seat-input`, `docket-core`, `prov`, `porter-core`, `porter-infer`, `model-replay` |
| `cuad` | `a11y-tree`, `cua-bus`, `cua-dbus`, `cua-desktop`, `cua-run`, `maskcap`, `seat-input`, `docket-core`, `docket-client`, `prov`, `porter-core`, `porter-infer`, `porter-client`, `almanac-core`, `almanac-client` |

The edges differ from SPEC 1.3 in one way, forced by its own types: `seat-input`, `maskcap` and
`a11y-tree` take `cua-desktop` as their base (a lease, a stamp, a seat's isolation and who masks
are named by both the desktop and the crates that serve it), and `cua-bus` takes the four leaf
crates, because the records and the `Start` body name their types. See FINDINGS, "Spec
conflicts".

The repo order is stoker, porter, almanac, docket, cua, sill. cua reaches stoker's
`cua-action` and `model-replay`, porter's `prov`, `porter-core`, `porter-infer` and
`porter-client`, almanac's `almanac-core` and `almanac-client`, and docket's `docket-core`
(`CuaAsk`, `GateAnswer`, `RunMode`, `WindowTrust`, `MessageDraft`) and `docket-client`
(`IntentProvider`, `Intents`), all by sibling path until pinned git revs replace them. Nothing of
sill may enter any tree here. cua never reaches `policy-point`: the router is the one decision
point and cuad asks it through `Intents1.Gate`.

External boundaries: see `scripts/check-boundary.sh`. `zbus` is behind `cua-dbus` and `cuad`
only; the pure set never reaches an effect crate; `wayland-client` is behind the `cosmic` and
`wayland` features, `reis` behind `ei` (and `cua-fake`'s `FakeEis`), `atspi` behind `atspi`.

## 2. Modules

| Crate | Modules |
| --- | --- |
| `cua-desktop` | `units`, `window`, `lease`, `caps`, `glow`, `event`, `error`, `traits`, `cosmic/` |
| `maskcap` | `pixels`, `mask`, `digest`, `settle`, `frame`, `capture`, `wayland/` |
| `seat-input` | `op`, `keymap`, `caps`, `plan`, `receipt`, `seat`, `region`, `ei/` |
| `a11y-tree` | `model`, `selector`, `ops`, `source`, `atspi/` |
| `cua-bus` | `ids`, `budget`, `state`, `action`, `task`, `report`, `record`, `bodies`, `control` |
| `cua-run` | `config`, `observation`, `effect_class`, `mode`, `turn`, `lease_book`, `stall`, `model`, `seams`, `machine/{phase, input, effect, run}` |
| `cua-dbus` | `names`, `error`, `manager`, `run`, `introspect` |
| `cua-fake` | `clock`, `seat`, `eis`, `capture`, `desktop`, `a11y`, `model`, `gate`, `events`, `harness` |
| `cuad` | `clock`, `config`, `callers`, `guard`, `backend`, `gate`, `model`, `links`, `provider`, `supervisor`, `system`, `serve` |

## 3. One home per concept

| Concept | Home |
| --- | --- |
| who acted, effects, labels, ids of sessions, runs, tasks, messages and entities, `Message`, `AgentRef`, `Address` | porter `prov` (never redefined here); the actor of a run's records is `cua_bus::cua_actor` |
| **a run's progress and result** | `prov::Message` of kind `Report`; `cua_bus::report_draft` builds the draft, `CuaOutcome::status` picks the status. There is no result type |
| coordinate spaces, `CuaAction`, `Point<S>`, `Chord`, `NodeId` | stoker `cua-action` (`WindowSpace`, never a bare pixel pair) |
| the model step (`CuaBegin`, `CuaStepRequest`, `CuaStepReply`, `TreeText`, `StepIndex`, `ServedBy`) | porter `porter-infer`; `cua_run::turn` builds the request |
| the gate contract (`CuaAsk`, `GateAnswer`, `EffectBasis`, `WindowClass`, `RunMode`, `WindowTrust`, `GrantAsk`, `MessageDraft`) | docket `docket-core`; `cua-desktop` re-exports `RunMode` and `WindowTrust` |
| the confirmation | the router's, through `Confirm1`; cua only stops its input (`RunEffect::SuspendLease(Consent)`, then `Proceed`) |
| the per-app consent | docket's `ActionGrant` with caller Cua, asked through `Gate.Grant` |
| desktop vocabulary, units (`Stamp`, `Ms`, `Digest32`), leases, backend capabilities | `cua-desktop` |
| frames, masks, digests, settling | `maskcap` |
| input operations, the plan, the keymap, the EI region map | `seat-input` (the region map is `seat_input::region`; nothing else names a global position) |
| the accessibility tree, selectors | `a11y-tree` |
| run states and slugs, outcomes, `ModeSupport`/`ModeGap`, the log records, `RunBudget`, control refusals and who may call which member | `cua-bus` |
| the machine, the phase table, effects, `mode_support`, `classify_effect` | `cua-run` |
| the proposed values | `cua_run::CuaConfig` and `SETTING_ROWS`; the keys are sill's |
| the D-Bus interface | `cua-dbus` and `dbus/org.quire.Cua1.xml` |
| the Wayland protocol | `protocols/quire-agent-v1.xml` and `quire-agent-protocol`; the fork's server side is `~/cosmic-comp` |
| the `cua.run.start` action | `manifests/org.quire.Cua.toml`, served by `cuad::CuaProvider` |
| the event-log record | `cua_bus::CuaRecord`; `cuad::record_of` makes the almanac `EventBody::Area { area: Cua }` |
| views of the above for the shell | sill's `sill-model` (`run_state_of(slug, outcome)`); cua defines none |

## 4. Traits

| Trait | Home | Implemented by |
| --- | --- | --- |
| `Desktop`, `LeaseControl` | `cua-desktop` | `CosmicDesktop`, `FakeDesktop` |
| `WindowCapture`, `CaptureSession` | `maskcap` | `StockToplevelCapture`, `QuireMaskedCapture` (through `CaptureBackend`), `FakeCapture` |
| `AgentSeat` | `seat-input` | `EiSeat` (through `SeatBackend`), `FakeSeat` |
| `A11ySource` | `a11y-tree` | `AtspiSource`, `FakeA11y` |
| `CuaModel`, `StepSink` | `cua-run` | `InferdCuaModel`, `ScriptedModel` |
| `GateLink`, `GateCheck` | `cua-run` | `IntentsGate`, `FakePdp` |
| `RecordSink`, `MessageLink` | `cua-run` | `DaemonLinks`, `MemoryEvents` |
| `Clock` | `cua-run` | `SystemClock`, `FixedClock` |
| `Seams` | `cua-run` | `SystemSeams`, `FakeSeams` |
| `IntentProvider` | docket `docket-client` | `CuaProvider` |

Closed sets stay enums: the backends (`BackendKind`, `SeatBackend`, `CaptureBackend`), every
machine state, input and effect, every record and body.

## 5. A run in one picture

```
Cua1.Manager.Start (router or planner only; Disabled while cua.enabled = off)
  -> Run::start: mode_support(caps, mode)        Unsupported  -> Finished(Blocked(ModeUnsupported))
  Granting     Gate.Grant(app, space)            Refused      -> Finished(Blocked(ConsentDenied))
  Leasing      LeaseControl::lease (one run per window; a Shell window is never leasable)
  loop per step:
    Observing  capture (masked: by cuad on stock, by the compositor on the fork) + a11y snapshot
    Thinking   CuaStep -> CuaStepReply, points in WindowSpace
    Authorising  classify_effect -> Gate.Check(CuaAsk)  (screen label always Untrusted)
       Confirming(id)  router will ask the person: stock stops its own input (SuspendLease(Consent)),
                       then Request.Proceed; the router shows the sheet; Verdict arrives
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
and in `Run::step`; (3) a table test in `cua-run/tests/machine.rs`; (4) the effect carried out
in `cuad::Supervisor`, and a fake behaviour in `cua-fake` if a seam changed.

**Add a backend.** A `BackendKind` variant and its row in `BackendCaps::of`; its rows in the
`mode_support` table test; a `CaptureKind` row in `cuad::capture_kind`; the `Desktop` and
`LeaseControl` implementation behind a feature of `cua-desktop`.

**Add a member to `Cua1`.** (1) The method or signal in `cua-dbus` (`manager.rs` or `run.rs`),
on the proxy and the skeleton; run `cargo test -p cua-dbus`, which prints the new XML; replace
`dbus/org.quire.Cua1.xml` with it in the same commit; (2) a `CuaMember` and its row in
`cua_bus::permits` (and the wildcard-free list test); (3) the body type in `cua-bus` with a
round-trip test; (4) its handler in `cuad::serve`.

**Add a record.** A `CuaRecord` variant with `#[serde(rename = "cua.<name>")]`, its row in
`tests/wire.rs` (the slugs are pinned), and its production in the machine. `cuad::record_of`
needs no change: the slug is the kind tag.

**Add a setting.** A field of `CuaConfig` with its default, a row in `SETTING_ROWS` and an arm of
`CuaConfig::value`; the test that pins the rows to the defaults fails until all three agree. The
design/22 row and the sill key are the other repos' agents'.

**Add a message-borne behaviour.** Don't: a message is input. If a run must react to one, the
reaction goes through `RunInput::Inbound`, changes no lease, mode, budget or permission, and every
action it leads to is gated like any other.

**Add a dev script.** Under `scripts/dev/`, with a private runtime dir, a private bus and scratch
`HOME`; never the person's `wayland-0` or bus. Acceptance scripts for the fork are
`accept-agent-seat.sh` and `accept-masking.sh`, run by hand with `COSMIC_COMP` set, never in CI.

## 7. Repo rules

- Licence `MIT OR Apache-2.0`; edition 2024; `unsafe_code = "deny"`; `rust-toolchain.toml` pins
  1.98.1 and `deny.toml` is the licence gate.
- Cross-repo dependencies are `path` entries to sibling checkouts (`../stoker`, `../porter`,
  `../almanac`, `../docket`) until each upstream freeze is merged and a pinned git rev replaces
  them. External versions are copied from quire's `docs/workspace-deps.toml`, never chosen here;
  FINDINGS lists the ones added to this workspace.
- Behaviour is `todo!()` behind frozen signatures, each listed in `FINDINGS.md`; shape tests
  (round trips, pinned JSON, introspection, tables) pass.
- Tests never touch the real system: no real seat, no real capture, no real compositor, no real
  bus, no real engine. The daemon's binary is a skeleton that exits with code 2.
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
  `cosmic`, `wayland`, `ei` and `atspi` backends compile with their features and are exercised
  only through fakes; the live checks are the dev scripts).
- The `dist/` units carry their sandbox lines (`cuad.service`: Unix sockets only, no network, no
  files, no devices, no capabilities); the D-Bus activation files name the two bus names cuad
  owns (`org.quire.Cua1`, `org.quire.Cua`).
