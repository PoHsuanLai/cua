# Findings

Open items and standing facts. An entry names the condition that closes it. At the freeze there
are **56 `todo!()` bodies** in library and daemon code (lines that begin with `todo!(`), listed
below; the tests contain none. 27 tests are `#[ignore]`d with the work that blocks them (a 28th regenerates the a11y fixtures on purpose), each with
real assertions. `cua-bus`, `cua-dbus` and `quire-agent-protocol` have none: their behaviour is
serde, tables and generated code.

## Stubs behind frozen interfaces

| Where | Count | Closes when |
| --- | --- | --- |
| a11y-tree `prune`, `render`, `diff`, `secret_rects`, `resolve` (`ops`) | 5 | the a11y-tree fill: `tests/behaviour.rs` (prune within budget, stable golden render that never prints a secret value, diff, secret rectangles; `resolve` with the procedures wave) |
| a11y-tree `AtspiSource::snapshot` (feature `atspi`) | 1 | after Spike E: find the window by pid and title, walk to the budget, window-local bounds |
| cua-desktop `CosmicDesktop::{windows, next_event, lease, release, suspend, resume, glow}` (feature `cosmic`) | 7 | the cua-desktop fill, after the fork: toplevel info and workspaces, `quire-agent-v1` leases (fork), `GetSenderSocket` with the `CosmicOSK` name workaround (stock) |
| maskcap `apply_masks`, `digest`, `settle`, `pixel_diff` | 4 | the maskcap fill: `tests/behaviour.rs` (exact device pixels at 1.5, digest, quiet/waiting/timeout table, pixel diff) |
| maskcap `StockToplevelCapture::open`, `QuireMaskedCapture::open`, `WaylandSession::{frame, damage}` (feature `wayland`) | 4 | the maskcap `wayland` fill: ext-image-copy-capture, 8-bit shm formats only |
| seat-input `plan` | 1 | the seat-input fill: `tests/plan.rs` |
| seat-input `Keymap::{parse_xkb, stroke_for, code_of}` | 3 | the seat-input fill (adds `xkbcommon`, below) |
| seat-input `EiSeat::{attach, perform, emulation, detach}` (feature `ei`) | 4 | the `ei` fill against `FakeEis` (`ei_seat_against_fake_eis`) |
| cua-run `Run::{start, step, wake}` | 3 | the run machine fill: `tests/machine.rs` |
| cua-run `classify_effect`, `turn`, `stalled` | 3 | the cua-run fill: `tests/tables.rs` |
| cua-run `LeaseBook::{claim, set_state, release}` | 3 | the cua-run fill: `lease_book_one_run_per_window` |
| cua-fake `FakeEis::serve` | 1 | with `EiSeat` |
| cua-fake `ScriptedModel::from_cassette` | 1 | with stoker's `cua-parse` fill |
| cua-fake `run_harness` | 1 | the run machine fill: `end_to_end_scripted_run_done` |
| cuad `IntentsGate::{grant, check}`, `IntentsCheck::{next, proceed}` | 4 | cuad fill 3, with docket-client's `DbusTransport` |
| cuad `InferdCuaModel::{begin, step}` | 2 | with porter's `DbusTransport::open` and the fd session (memfd frames) |
| cuad `DaemonLinks::{record, send, inbox}` | 3 | cuad fill 3: `record_of` (built) then `Memory::record`; `Intents::send`/`inbox` |
| cuad `CuaProvider::perform` | 1 | cuad fill 3: `cua.run.start` into a `CuaTask` (`choose_mode` resolves `auto`) |
| cuad `Supervisor::{start, control, answer, stop_all}` | 4 | cuad fill 3: `end_to_end_scripted_run_done`, `kill_chord_finishes_all_runs`, `stock_consent_guard_injects_nothing_while_prompt_shown` |
| cuad `serve` | 1 | cuad fill 3: one handler per `Cua1` member |
| daemon binary | | a skeleton that exits 2 |

Per crate: a11y-tree 6, cua-bus 0, cua-desktop 7, cua-dbus 0, cua-fake 3, cua-run 9, cuad 15,
maskcap 8, quire-agent-protocol 0, seat-input 8; total 56.

What is built, not stubbed: `BackendCaps::of`, `LeaseRequest::check`, glow, revoke, suspend and
mode wire codes (pinned against the generated protocol enums under the `cosmic` feature),
`SeatCaps::for_isolation` (S10: stock offers no Unicode text path), `RegionMap`, `MaskReason::code`,
`mode_support` and `choose_mode`, `CuaConfig` with `SETTING_ROWS`, `RunPhase` facts, the
`ModeSupport` and `Modes`/`Backend` wire strings, `CuaOutcome::status`, `permits`, `report_draft`,
`ControlRefusal` names, `cuad::guard`, `CallerNames::classify`, `manifest_for`, `record_of`,
`capture_kind`, `CuadConfig::from_toml`, and the fakes other than the stubs above.

## Spec conflicts and how they were resolved

- **Results are `prov::Message` Reports (user decision, one message model).** SPEC 3.9 lists
  `CuaResult` in cua-bus; there is none. `CuaOutcome` stays as the log's detail and
  `CuaOutcome::status()` picks the report's status; `RunEffect::Report(MessageDraft)` is sent through
  `Intents1.Message.Send` (the cua role may send and read the inbox); `Cua1.Run.Finished` carries a
  `RunEnd` (outcome and which message carries the report). `CuaTask` gains `session` and
  `report: ReportRoute`. `CuaRecord::Finished` links the message id. `RunInput::Inbound` takes
  messages in and widens nothing.
- **A message carries no authority.** No input grants a lease, mode, budget or permission;
  `a_message_changes_no_authority` pins it (ignored until the machine).
- **Edges.** SPEC 1.3 gives seat-input, maskcap and a11y-tree only `cua-action`, and cua-bus only
  prov, docket-core and porter-infer. Its own types need more: a lease, a `Stamp` and `SeatIsolation`
  are named by `AgentSeat::attach`, `WindowCapture::open` and `BackendCaps`; the records name
  `InputReceipt`, `FrameDigest`, `NodeSelector`. So `cua-desktop` is the base crate and the leaf
  crates depend on it; cua-bus depends on the four leaves; `RunBudget` lives in cua-bus (SPEC 2 says
  cua-run) because `CuaTask` carries it. ARCHITECTURE.md has the exact table, enforced.
- **`LeaseGrant` is `Eq` data.** The EIS descriptor travels as `Leased { grant, socket: EiSocket }`;
  `AgentSeat::attach(&LeaseGrant, EiSocket)`.
- **Authorising, confirming and the grant follow docket's gate, not cua.md.** `Gate.Check` returns a
  Request; its `Progress(Confirming(id))` becomes `RunInput::Confirming`, then stock suspends the
  lease and calls `Proceed`, and `Verdict(GateAnswer)` ends it. `ConfirmText`, `ShowConsent`,
  `DismissConsent`, `ConsentAnswered`, `DenyReason`, `CuaVerdict` and the per-run `denials` budget
  are gone (C10, C12, C14). `Gate.Grant` resolves inside the router: no `ShowGrantPrompt`.
- **`ModeGap::NoHiddenInput` (Spike A).** Stock renders hidden windows but delivers them no input and
  has no agent seat: `AgentWorkspace` is unsupported there. The mode table is pinned.
- **Stock facts built into the types.** `Frame.scale` is device pixels per logical pixel (120 on
  stock at any output scale, since capture is at logical resolution); `Frame.damage` is the whole
  window on stock and settling uses `pixel_diff` (`DamageSource::PixelDiff`); capture asks for
  8-bit shm formats; `TextPath::EiText` exists but stock does not offer it (S10);
  `ConsentGuard::CuadSuspendsInput` and `MaskWhere::Cuad` for stock.
- **`cua.default_mode`** is `auto` (U11), not design/22's `agent_workspace`; `cua.budget.denials`
  has no row (the router's breaker counts).
- **`Cua1.Run.Finished` argument** is named `end` (body `RunEnd`), not `result`.
- **Records:** `Routed` and `Asked.question_id` were added; `HandedBack` has no `user_events`
  (almanac types are outside cua-bus's edges).
- **`MaskedRegion` is `MaskRegion`**; `ScrollSteps` is `Notches` with a `ScrollDir`; `PxSize` is
  `DeviceSize`.
- **`Seams` and `Parts` live in cua-run**, not cuad, so `cua-fake` can implement the bundle.
- **`Run::start` takes a `RunEnv`** (backend caps and settings), and `CuaTask` an `app`; a run
  targeting `TaskTarget::App` ends `Blocked(NeedsCapability)` until launch-into-mode lands.
- **cuad reaches `cedar-policy` through docket-client** (upstream asks); it has no edge to
  `policy-point`, which the edge table enforces.

## Dependencies added to this workspace (not in quire's pinned block)

- `wayland-scanner 0.31`, `wayland-server 0.31`, `wayland-backend 0.3`: `quire-agent-protocol`
  generates its own bindings; all are already in every wayland tree.
- `atspi` is pinned with `default-features = false, features = ["connection", "proxies", "tokio"]`
  (the block's bare `0.30` cannot be narrowed by a member); `reis` gains `tokio` in seat-input;
  `base64 0.22` is already in the block (maskcap's serde form).
- Not yet added, closed by their fills: `xkbcommon 0.8` (seat-input `Keymap::parse_xkb`),
  `cosmic-client-toolkit` (cua-desktop `cosmic`), `ashpd`, `pipewire` (KWin).

## Upstream asks (nothing was edited in another repo)

- **stoker `cua-action`:** the markers `WindowSpace`, `ImageSpace`, `GridSpace` derive neither `Hash`
  nor `Ord`, so `Point<S>`, `Rect<S>` and `CuaAction<S>` are not `Hash`/`Ord`. Ask:
  `#[derive(Hash, PartialOrd, Ord)]` on the three markers.
- **stoker `cua-action`:** `PixelFormat` lacks `Abgr2101010`/`Xbgr2101010` (stock offers them);
  not needed, noted.
- **porter `porter-infer`:** `CuaStepRequest` has no field for messages that arrive mid-run. Ask:
  `pub notes: Vec<StepNote>` with a labelled text `StepNote(String)`; until then `RunInput::Inbound`
  is queued and not shown to the model.
- **docket message label:** a cua report's label must include `Untrusted(Screen { app })`, but
  `MessageDraft` has no label. Ask: the router joins the `screen` label of every `CuaAsk` it has
  seen for a run into any message from `AgentRef::Cua { run }`, or `MessageDraft.floor: Label`.
- **docket `docket-client`:** `Intents` has no `gate_grant`/`gate_check`; cuad calls
  `Transport::call` with `IntentsRequest::{GateGrant, GateCheck}`. Optional convenience.
- **docket `docket-client`:** `InProcess` links `docket-router`, hence `policy-point` and
  `cedar-policy`, into every client. Ask: put it behind a feature so cuad honours SPEC C10 in its
  tree.
- **almanac:** `HandedBack.user_events` needs `EventRef` in cua-bus's edges or a memory call at
  record time; memory fill decision.
- **cosmic-comp fork:** serve `quire_agent_manager_v1` from `protocols/quire-agent-v1.xml`; the
  `mask_reason`, `glow_state`, `suspend_reason`, `revoke_reason`, `lease_mode` values are pinned by
  tests here.

## Decisions beyond the spec

- `cua-desktop` as the base crate; `Digest32` for `FrameDigest` and `ThoughtDigest`; `Ms`, `Stamp`
  (cuad clock, never wall time).
- The run reports `Progress` only while it waits for the person (a question, a pause).
- `cua.run.start` returns the run id once the run has begun (`latency = long`: it waits for the
  consent and the lease); its report arrives as a message. Its manifest is `reach = hidden` while
  `cua.enabled = off` and `offered` once on (`manifest_for`).
- `Cua1` caller classes `Router`, `Companion`, `Shell`, `Other`, derived from bus-name ownership
  (`CallerNames`); `Start` is the router's and the planner's, `Answer` the shell's alone.
- `Emulation::{Running, Stopped}` on `AgentSeat` (suspend and resume in one method, six methods in
  all); on stock it keeps cuad's input out of a confirmation.
- `seat_input::region` is a public module (its `GlobalPoint` is named nowhere else).
- Acceptance scripts `scripts/dev/accept-{agent-seat,masking}.sh` are stubs: they check
  preconditions (`COSMIC_COMP`, the harness) and exit 2, with the pass criteria written in them.
- `Label::{trusted_user, untrusted, join}` are `todo!()` in prov, so tests use `Label` literals and
  machine tests that would need them are ignored.
- Nothing was pushed; no GitHub repo.
