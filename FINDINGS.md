# Findings

Open items and standing facts. An entry names the condition that closes it. `cua-bus`,
`cua-dbus` and `quire-agent-protocol` have no `todo!()`: their behaviour is serde, tables and
generated code.

## Stubs behind frozen interfaces

At the fill of 2026-10-03 (lane w4-cua) **18 `todo!()` bodies** are left in library and daemon
code (lines that begin with `todo!(`); the tests contain none. All of them are the backends that
need a compositor, a seat or a bus that cannot be had in a test; every one of the 51 that were
behaviour over fakes is built. Two tests are `#[ignore]`d on purpose: they regenerate fixture
files (`a11y-tree` fixtures and golden renders).

| Where | Count | Closes when |
| --- | --- | --- |
| a11y-tree `AtspiSource::snapshot` (feature `atspi`) | 1 | after Spike E: find the window by pid and title, walk to the budget, window-local bounds |
| cua-desktop `CosmicDesktop::{windows, next_event, lease, release, suspend, resume, glow}` (feature `cosmic`) | 7 | the cua-desktop fill, after the fork: toplevel info and workspaces, `quire-agent-v1` leases (fork), `GetSenderSocket` with the `CosmicOSK` name workaround (stock) |
| maskcap `StockToplevelCapture::open`, `QuireMaskedCapture::open`, `WaylandSession::{frame, damage}` (feature `wayland`) | 4 | the maskcap `wayland` fill: ext-image-copy-capture, 8-bit shm formats only |
| seat-input `EiSeat::{attach, perform, emulation, detach}` (feature `ei`) | 4 | the `ei` fill against `FakeEis` (`ei_seat_against_fake_eis`) |
| cua-fake `FakeEis::serve` | 1 | with `EiSeat` |
| cua-fake `ScriptedModel::from_cassette` | 1 | with stoker's `cua-parse` fill |
| daemon binary | | a skeleton that exits 2: it needs `SystemSeams` (`CosmicDesktop`, `CaptureBackend`, `SeatBackend`, `AtspiSource` over their backends); `cuad::Links::over` builds the three daemon links from one connection |

Per crate: a11y-tree 1, cua-desktop 7, cua-fake 2, maskcap 4, seat-input 4; total 18.

What is built, not stubbed (all behind the frozen signatures): `BackendCaps::of`, `LeaseRequest::check`,
glow, revoke, suspend and mode wire codes (pinned against the generated protocol enums under the
`cosmic` feature), `SeatCaps::for_isolation` (S10: stock offers no Unicode text path), `RegionMap`,
`MaskReason::code`, `mode_support` and `choose_mode`, `CuaConfig` with `SETTING_ROWS`, `RunPhase`
facts, the `ModeSupport` and `Modes`/`Backend` wire strings, `CuaOutcome::status`, `permits`,
`report_draft`, `ControlRefusal` names, `cuad::guard`, `CallerNames::classify`, `manifest_for`,
`record_of`, `capture_kind`, `CuadConfig::from_toml`, and the fakes other than the stubs above; and,
from the w4-cua fill: the a11y-tree functions (`prune`, `render`, `diff`, `secret_rects`,
`resolve`), maskcap (`apply_masks`, `digest`, `settle`, `pixel_diff`), `Keymap::parse_xkb`, cua-run
(`classify_effect`, `stalled`, `LeaseBook`, `turn`, `note_of`, the machine `Run::{start, step,
wake}`, and `Exec`, which carries a run's effects out against the seams), `run_harness`, and cuad
(`IntentsGate`, `DaemonLinks`, `Supervisor`, `serve`/`serve_on`, `CuaProvider::perform`, `Links`).

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
  lease and calls `Proceed`, and `Verdict(GateAnswer)` ends it (the machine and the seams are built
  that way; docket-client does not hand the progress out yet, see the asks). `ConfirmText`, `ShowConsent`,
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
- **Records:** `Routed` and `Asked.question_id` were added; `HandedBack.user_events` is
  `Vec<EventRef>` (cua-bus depends on almanac-core, ask 72; the machine leaves it empty, the
  events of the person between the take-over and the hand-back are memoryd's to name once a seam
  carries them).
- **`MaskedRegion` is `MaskRegion`**; `ScrollSteps` is `Notches` with a `ScrollDir`; `PxSize` is
  `DeviceSize`.
- **`Seams` and `Parts` live in cua-run**, not cuad, so `cua-fake` can implement the bundle.
- **`Run::start` takes a `RunEnv`** (backend caps and settings), and `CuaTask` an `app`; a run
  targeting `TaskTarget::App` ends `Blocked(NeedsCapability)` until launch-into-mode lands.
- **cuad has no edge to `policy-point`** (the edge table enforces it), and no longer reaches
  `cedar-policy` either: docket-client's `InProcess` is behind its `in_process` feature, which
  only cuad's tests turn on, through the dev-dependency on intentd.

## Dependencies added to this workspace (not in quire's pinned block)

- `wayland-scanner 0.31`, `wayland-server 0.31`, `wayland-backend 0.3`: `quire-agent-protocol`
  generates its own bindings; all are already in every wayland tree.
- `atspi` is pinned with `default-features = false, features = ["connection", "proxies", "tokio"]`
  (the block's bare `0.30` cannot be narrowed by a member); `reis` gains `tokio` in seat-input;
  `base64 0.22` is already in the block (maskcap's serde form).
- `rustix 1.1` (`std`, `fs`), porter's line narrowed: cuad makes the frame memfd (`memfd_create` and its seals).
- `xkbcommon 0.8` with `default-features = false` (seat-input `Keymap::parse_xkb` reads the text
  of a keymap and no wayland keymap fd; the block's bare line enables `wayland` and `memmap2`);
  `blake3 1` (maskcap's frame digest and cua-run's thought digest; already in the lock); `futures-util`
  on docket's line (cuad waits for its connection to close).
- Dev-dependencies of cuad's tests only, so not in any tree the boundary script checks: `intentd`,
  `docket-fake`, `docket-router` (a real intentd over docket's fakes on a private bus) and
  `almanac-fake` with `almanac-client`'s `in_process` (a real memory service); `tokio`'s `test-util`.
- Not yet added, closed by their fills: `cosmic-client-toolkit` (cua-desktop `cosmic`), `ashpd`,
  `pipewire` (KWin).

## Upstream asks (nothing was edited in another repo)

Closed by the w4-cua fill, from the asks lists: 62 (cuad calls `Intents::gate_grant` and
`gate_check`), 72 (`HandedBack.user_events`, cua-bus on almanac-core), 92 (`Observation.notes`), the
docket label ask (the router joins `Untrusted(Screen)` into the run's session at `Gate.Check`), the
docket `InProcess` feature ask (it is behind `in_process`), and the porter `StepNote` ask
(`CuaStepRequest.notes` exists).

- **stoker `cua-action`:** the markers `WindowSpace`, `ImageSpace`, `GridSpace` derive neither `Hash`
  nor `Ord`, so `Point<S>`, `Rect<S>` and `CuaAction<S>` are not `Hash`/`Ord`. Ask:
  `#[derive(Hash, PartialOrd, Ord)]` on the three markers.
- **stoker `cua-action`:** `PixelFormat` lacks `Abgr2101010`/`Xbgr2101010` (stock offers them);
  not needed, noted.
- **docket `docket-client`:** `Intents::gate_check` waits for the router's final answer inside one
  call and hands out neither the Request's `Progress(Confirming(id))` nor `Proceed`, and the router
  does not implement `Request.Proceed` yet. So a check here never says `Confirming`: the stock
  consent guard (cuad stops its own input before the router's sheet shows) is built in the machine
  and the seams but never triggered by the real gate, `CuaRecord::Step.authorised_by` cannot say
  `User`, and a run that is paused during a confirmation cannot withdraw it (the call is aborted;
  the router's Request waits out its expiry). cuad injects one action at a time and only after the
  verdict, so its input is idle while the router asks. Ask: `Intents::gate_check_watched(ask, on_confirming)`
  that reports the `ConfirmId` and returns a handle with `proceed()` and `close()`.
- **docket router / spec:** no one can open the session a run's steps are ruled in. `Gate.Check`
  finds the session whose actor is `Companion { role: Cua { run } }` and answers `Halted(Any)`
  without it; `Session.Open` is for the launcher, field and companion roles (not cua), and the run
  id is minted by cuad after `cua.run.start`. cuad's tests open the session as the companion does,
  after the id is known, and the first `Gate.Check` of a real run races that. Ask: let the cua role
  open the session of its own run (`Session.Open { agent: Cua { run } }`, only for a run it started),
  or have `cua.run.start`'s perform open it and return both. `CuaTask.session`, `Message.Send` and
  every record carry that session, so the same answer fixes the sender of the reports (today a
  report sent in the planner's session would go out as the planner).
- **docket `Run.Perform` / Intents1 XML:** `inv.actor` carries the planner's session, which
  `cua.run.start` uses as the task's session until the ask above is settled (ask 62: any copy of
  the XML gains `session` on `Run.Perform` and `app` on `Context.Current`).
- **porter `porter-infer` / cuad `Seams`:** a run holds a model session (`CuaBegin` once, then
  steps), but `Seams` has one `Model`. The supervisor therefore lets a run hold the model for as
  long as it lasts and runs on different windows take turns at their first model call. Ask: a
  per-run model (`Seams::Model` becomes a factory, `fn open(&self) -> Self::Model`), so runs on
  different windows think at once.
- **docket / cua context:** the `Context` call of `Intents1` answers a planner's view, not the
  `ContextSnapshot` of the window the run works in, so `Observation.context` is always `Absent`
  (`RunEffect::FetchContext` is carried out as "no answer"). Ask: a `Context` member for the cua
  role that returns the snapshot of an app's window.
- **almanac `Memory1`:** `HandedBack.user_events` needs a read of the person's events between two
  instants for the cua caller; `Caller::Cuad` records `Area(Cua)` only. Ask: a bounded `Recent`
  for `Caller::Cuad`, limited to events of the window's app after a given `Seq`.
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

## Following porter's client fill (ask 82)

- `InferdCuaModel::{begin, step}` are built over `Transport::open(Need::ComputerUse, Screen, tier)`
  (the tier is `Balanced`, `on_tier` sets `CuadConfig::tier`). `begin` sends `CuaBegin` and reads
  to the empty `CuaStepReply` that acknowledges it (a reply with actions in it is `Fatal`). `step`
  puts the frame in a sealed memfd (`frame_fd`: written whole, offset back at 0 because
  SCM_RIGHTS shares it, sealed against writes and resizing), sends `CuaStep` with
  `send_attached(.., vec![fd])`, forces `FrameImage.source` to `Attached(AttachIndex(0))` and the
  layout to the buffer's own, forwards `ThoughtDelta` and `ActionProposed` to the sink and ends on
  `Finished`. A model failure leaves the session usable; a closed or broken session is dropped and
  every later step is `Fatal` (the history it was pinned to is gone). Sorting: unreachable
  inferd, an engine not ready or rate limited, `Refused(Unavailable)` and `Cancelled` are
  `Transient`; unreadable and unparseable replies are `Parse`; every other refusal and failure is
  `Fatal`. Tested over a scripted session that reads the descriptor back (`cuad/tests/model.rs`).
- `seat_input::plan` is filled (it was `todo!()`, and `Keymap::{stroke_for, code_of}` with it). A
  `Target::Centre` is `centre_of(action.window)` (half of each side, rounded down; the caller's
  `ResolvedPoints` never overrides it); a node is the point the caller resolved (`PlanError::
  Unresolved` when it did not); every point is checked against the window. The old chord test
  expected key code 49 for `s`; the test keymap lays `a` at 30, so `s` is 48.
- `CuaStepRequest.notes` is carried by the fakes' literals (`cua-fake` `blank_request`), by
  `InferdCuaModel` (it sends the request as given) and, since the w4-cua fill, by `cua_run::turn`
  (see below).

## The w4-cua fill (2026-10-03): decisions the specs left open

The pure crates and the seams were filled together because the daemon cannot be tested without
the machine under it. What was decided:

- **`Run` gains `track: RunTrack`.** The frozen struct had no home for the capture retry, the
  step's outcomes so far (they become the next observation's `prev`), the settle watch (what a
  `Settling` phase waits for: damage since an action, or a timer for `wait`), who authorised the
  action being carried out, the thought digest and frame note of the step, and the rate window.
  `tests/machine.rs` builds `Run` literals and carries `track: RunTrack::default()`.
- **Machine rows.** Granting asks the router; `Leased` records `RunStarted` and starts the first
  look. A look is `Snapshot`, `FetchContext` (first look and after a hand-back), then `Capture`,
  which is always last: the capture is what builds the observation. The model's batch is a queue:
  pointer and keyboard actions go to the router one at a time (a target outside the window or an
  unknown node is refused locally and never asked), `wait` pauses for `min(asked, cua.wait.max_ms)`,
  `zoom` and `observe` look again and drop the rest (the crop hint is not used), `ask` waits for
  the person (one `QuestionId` per step, a `Progress` report to the requester), `finish` ends the
  run (`Failed` and `Infeasible` both end `Failed(Infeasible)`; a success check on the tree must pass
  first). Empty replies count as model failures and look again at the next step; transient and
  parse failures look again at the same step; over `cua.budget.model_failures` ends `Failed(Model)`.
  A vendor safety hint of either kind blocks the batch and counts as a failure (the router has no way
  to be told "confirm this", so the run does not go on without it). A refused action marks the rest
  `NotExecuted` and reaches the model through `prev`; `Unconfirmed` is told as `UserDeclined`;
  `Paused` ends `Blocked(PolicyDenials)`; `Halted` pauses the run; a router that cannot be asked
  blocks the run (fail closed). Active time is counted between inputs in active phases only (a
  message moves no clock); the rate limit is strict (`actions_per_min` of 0 allows none); spend is
  over when it is strictly greater (a local model costs nothing).
- **Reports go before the record of the end**, so `CuaRecord::Finished.report` can name the message
  that carried them (`Exec` fills it). A report for `Progress` is sent only when the run waits for
  the person: a question, a pause.
- **`turn`**: the tree is rendered (children indented, secrets as `<hidden>`, names escaped so a
  name can never end its line and pass for another node), the masked count is each region and the
  whole-frame mask as one more, the router's reasons are never told to the model (`Refused("not
  allowed")`), the person's answers to the model's own questions become `StepNote`s from the person
  (or from an agent when the label does not call them the person's), and `note_of` turns an
  inbound message into a note from its sender, plain text parts only (a handle is not text the run
  may read).
- **`Exec` (cua-run) carries one run's effects out against the seams and awaits nothing but
  seam futures**, so the harness and the supervisor drive the same code; they differ in how they
  wait. The tree is taken before the capture so the masks come from the tree of that very frame.
  The inbox is read at each look: each message is an input to the machine (which changes nothing by
  it) and a note for the next turn. `Suspend` stops the seat's emulation as well as the lease.
  `RecordSink` gained a defaulted `open(run, task)` so a sink learns a run's session and Space even
  when the run ends before it has a window.
- **Supervisor.** One task per run. Each effect is raced against the person's controls, the
  desktop's events and the clock; an interruption drops the in-flight work and the work queued
  behind it (records, signals and lease handling still happen). The desktop's events go to every run
  and each machine keeps what is its own (its lease, its window, the kill chord is everyone's). The
  supervisor owns the model in a lock (`Supervisor::parts` is replaced by one accessor per seam).
  `start` refuses with `NotPermitted`, `Disabled`, `ModeUnsupported`, `WindowUnknown` and `LeaseHeld`
  before any id is used.
- **`serve`.** The Manager and a Run per live run are served with the signatures of the skeletons
  (a unit test holds their introspection to `dbus/org.quire.Cua1.xml`); bodies are JSON in the
  `CuaEnvelope`; the caller's role is the well-known names its connection owns; `Answer` takes the
  person's own words as plain text and labels them trusted. A run's `Finished` carries the `RunEnd`,
  sent once the report is out. `serve_on(connection, supervisor)` serves on any connection (the
  tests use a private bus); `serve` connects to the session bus and waits for it to close.
- **`CuaProvider::starting`** returns the provider and a queue of `StartRequest`s; `serve_on` turns
  each into a `CuaTask` (the window's app is the window's, `auto` is resolved by the setting and the
  backend, the report goes back to the requester) and a run. `CuaProvider::new` still starts
  nothing.
- **`FakeDesktop::next_event`** wakes when an event is raised (it used to wait forever if none was
  queued when it was first polled). `cua_fake::scene` holds the scenes the tests share.
- **Tests.** `cuad/tests/links.rs` runs `IntentsGate` and `DaemonLinks` against a real intentd on a
  private bus (docket's fakes behind it) and a real memory service (almanac's fakes);
  `cuad/tests/serve.rs` runs `serve_on` on a private bus with callers that own the role names;
  `cuad/tests/{supervisor,inflight}.rs` run the supervisor over the fakes with paused time.
- **Not done:** `CuaProvider` offers no `dry_run`, search or suggest; procedures
  (`ProcedureUse::{Hint, Replay}`) are carried and not used; `data_in` is not sent to the model;
  there is no price for a cloud model (`ModelReply.cost` is zero), so the spend budget only guards
  a free local model.

