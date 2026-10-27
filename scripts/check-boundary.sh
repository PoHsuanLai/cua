#!/usr/bin/env bash
# Crate layers, mechanically enforced (ARCHITECTURE.md section 1 is the table; this is its
# mechanical form).
#
#   scripts/check-boundary.sh               run every check
#   scripts/check-boundary.sh --long-files  also report source files over 500 lines (no failure yet)
#   scripts/check-boundary.sh --self-test   seed violations and assert each is caught
#
# Every crate is one tier:
#   P  pure: no std::fs/env/process/net, no clock reads, no sleeps, no tokio, hyper, reqwest, zbus
#      or rustix, and no UI words.
#   C  client/IO: may do IO, but has no UI words (Help, Sheet, SettingRow, ds_settings).
#   A  app/daemon: wiring only. Its libraries read no environment variable outside main.rs.
# A crate depends on its own tier or a lower one (P < C < A), never up.
#
# `Label` is not a UI word here: prov::Label is the taint label and Role::Label is an
# accessibility role; neither is copy shown to a person.
#
# `cargo tree -i <dep>` exits 101 when the dependency is absent, which is precisely the state
# we want. Checking the exit status would therefore fail whenever the boundary holds, so we
# check for OUTPUT instead: any line naming the dependency is a leak.
set -uo pipefail
cd "$(dirname "$0")/.."

# LAYERS: crate -> tier. Every workspace member has exactly one row.
LAYERS=(
  "cua-desktop: P"
  "cua-types: P"
  "maskcap: P"
  "seat-input: P"
  "a11y-tree: P"
  "cua-bus: P"
  "cua-run: P"
  "cua-host: C"
  "cua-dbus: C"
  "cua-fake: C"
  "cua-eval: A"
  "cua-eval-cloud: A"
  "cuad: A"
)

# KNOWN: a violation that exists today, with the plan lane (~/rs-wt/agent-layers/PLAN.md) that
# removes it. "crate: ban: lane". A row whose violation is gone fails, so the list only shrinks.
# The P crates a11y-tree, cua-desktop, maskcap and seat-input hold their backend in a feature
# module (`atspi`, `wayland`, `ei`); the backends leave with cua-system (C0.8). cuad and
# cua-eval-cloud read the environment inside a library module instead of main.rs.
# `Role::SheetHost` is a docket-core wire name; cua names it only in test code, which the bans
# skip, so it needs no row.
KNOWN=(
  "a11y-tree: tokio: C0.8"
  "a11y-tree: zbus: C0.8"
  "cua-desktop: tokio: C0.8"
  "maskcap: clock: C0.8"
  "maskcap: rustix: C0.8"
  "maskcap: tokio: C0.8"
  "seat-input: clock: C0.8"
  "seat-input: fs: C0.8"
  "seat-input: net: C0.8"
  "seat-input: tokio: C0.8"
  "cuad: env-var: C0.7"
  "cua-eval-cloud: env-var: C5.3"
)

# BANS: "name|tiers|extended regex" over the non-test source of a crate of those tiers. A line
# that starts with `//` is skipped, and so is everything from the first `#[cfg(test)]` on.
BANS=(
  'fs|P|std::(fs|\{[^}]*\bfs\b)'
  'env|P|std::(env|\{[^}]*\benv\b)'
  'process|P|std::(process|\{[^}]*\bprocess\b)'
  'net|P|std::(net|os::unix::net|\{[^}]*\bnet\b)'
  'clock|P|SystemTime::now|Instant::now'
  'sleep|P|thread::sleep'
  'tokio|P|\btokio\b'
  'hyper|P|\bhyper\b'
  'reqwest|P|\breqwest\b'
  'zbus|P|\bzbus\b'
  'rustix|P|\brustix\b'
  'ui-words|PC|\b(Help|Sheet|SettingRow)|ds[-_]settings'
  # Outside main.rs and the src/bin programs; the environment is read once, at the entry point.
  'env-var|A|\benv::(var|var_os|vars|vars_os)\b'
)

# RULES: what a crate reaches through ANY path (transitive, default features).
#
# The pure crates (cua-desktop, maskcap, seat-input, a11y-tree, cua-bus, cua-run) never reach a
# bus, a runtime, an HTTP client, a wayland connection, an EIS client, an accessibility stack,
# a database, a file watcher, an embedding runtime, the MCP SDK or the policy engine: their
# backends (`wayland`, `ei`, `atspi`) are features, and the default tree is what is
# checked. `toml` is parsed at the edge (cuad), never below it.
EFFECTS="zbus zvariant tokio reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend wayland-server reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy toml"
RULES=(
  "cua-desktop: $EFFECTS"
  "maskcap: $EFFECTS"
  "seat-input: $EFFECTS"
  "a11y-tree: $EFFECTS"
  # The bus bodies: serde, plus the vocabulary crates named in ONLY below. The capability crates
  # (accessibility, input, capture) are not reachable, so a consumer that reads the bodies
  # (sill, the D-Bus codec) links none of them; the leaf types they share live in cua-types.
  "cua-types: $EFFECTS a11y-tree seat-input maskcap"
  "cua-bus: $EFFECTS a11y-tree seat-input maskcap"
  "cua-run: $EFFECTS"
  # The run executor as a library: a runtime and the docket/almanac client traits, but no bus
  # (their `dbus` features are the daemon's), no backend and no file watcher.
  "cua-host: zbus zvariant reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend wayland-server reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # zbus (with its tokio feature) lives in cua-dbus; no wayland, no EIS, no accessibility stack.
  "cua-dbus: reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend wayland-server reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # The fakes reach reis (the in-process EIS server) and, through model-replay, an HTTP stack;
  # never a bus, a wayland connection, the accessibility stack or a database.
  "cua-fake: zbus zvariant wayland-client wayland-backend wayland-server atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # The eval is a tool, not a daemon: it reaches the fakes (reis), a runtime and, for the live path,
  # an HTTP client to an engine on this machine; never a bus, a wayland connection, the
  # accessibility stack, a database or the policy engine.
  "cua-eval: zbus zvariant wayland-client wayland-backend wayland-server atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # The cloud eval is the eval plus the bus to inferd (zbus through porter-client): a stoker Provider whose
  # turns are inferd chat turns, so a hosted model's key stays in accountd. It reaches no wayland
  # connection, accessibility stack, database or policy engine.
  "cua-eval-cloud: wayland-client wayland-backend wayland-server atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # The daemon reaches the bus, a runtime, wayland, EIS and AT-SPI. It never reaches HTTP,
  # an embedding runtime, the MCP SDK or the policy engine. One decision point lives in the router
  # and cuad asks it through `Intents1.Gate` (SPEC C10): cuad has no edge to `policy-point`
  # (LAYERS above), and docket-client's `InProcess` (which links docket-router, hence cedar) is
  # behind its `in_process` feature, which only cuad's tests turn on (a dev-dependency).
  "cuad: reqwest hyper hyper-util rustls pipewire ort fastembed rmcp cedar-policy"
)

tier_of() {
  local row
  for row in "${LAYERS[@]}"; do
    [ "${row%%:*}" = "$1" ] && { echo "${row#*: }"; return; }
  done
}
rank() { case "$1" in P) echo 0 ;; C) echo 1 ;; A) echo 2 ;; *) echo 9 ;; esac; }

# Prints "file:line: text" for each non-test source line of crate $2 under root $1 that matches
# regex $3. A file whose path matches the regex $4 is skipped.
scan_source() {
  local root="$1" crate="$2" regex="$3" skip="${4:-^$}" file
  [ -d "$root/crates/$crate/src" ] || return 0
  while IFS= read -r file; do
    [[ "$file" =~ $skip ]] && continue
    sed '/^[[:space:]]*#\[cfg(test)\]/q' "$file" | grep -nE "$regex" \
      | grep -vE '^[0-9]+:[[:space:]]*//' | sed "s#^#$file:#"
  done < <(find "$root/crates/$crate/src" -name '*.rs' ! -name tests.rs ! -path '*/tests/*' | sort)
}

# Runs every source ban on crate $2 under root $1; prints "ban: hit" lines.
source_violations() {
  local root="$1" crate="$2" tier spec name tiers regex skip
  tier=$(tier_of "$crate")
  for spec in "${BANS[@]}"; do
    name="${spec%%|*}"; spec="${spec#*|}"
    tiers="${spec%%|*}"; regex="${spec#*|}"
    case "$tiers" in *"$tier"*) ;; *) continue ;; esac
    skip='^$'
    [ "$name" = env-var ] && skip='/(main\.rs|bin/[^/]*)$'
    scan_source "$root" "$crate" "$regex" "$skip" | sed "s#^#$name: #"
  done
}

known_lane() { # crate ban -> lane, empty when not listed
  local row
  for row in "${KNOWN[@]}"; do
    case "$row" in "$1: $2: "*) echo "${row##*: }"; return ;; esac
  done
}

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  LAYERS=("p-crate: P" "c-crate: C" "a-crate: A")
  for c in p-crate c-crate a-crate; do mkdir -p "$tmp/crates/$c/src"; done
  printf 'pub fn f() { let _ = std::fs::read("x"); }\n' >"$tmp/crates/p-crate/src/lib.rs"
  printf 'pub fn f() { let _ = std::fs::read("x"); }\npub struct SettingRow;\n' >"$tmp/crates/c-crate/src/lib.rs"
  printf 'pub fn f() { let _ = std::env::var("X"); }\n' >"$tmp/crates/a-crate/src/lib.rs"
  printf 'fn main() { let _ = std::env::var("X"); }\n' >"$tmp/crates/a-crate/src/main.rs"
  p_hits=$(source_violations "$tmp" p-crate)
  c_hits=$(source_violations "$tmp" c-crate)
  a_hits=$(source_violations "$tmp" a-crate)
  grep -q '^fs: ' <<<"$p_hits" || { echo "SELF-TEST FAILED: a std::fs call in a P crate was not caught"; exit 1; }
  grep -q '^fs: ' <<<"$c_hits" && { echo "SELF-TEST FAILED: a std::fs call in a C crate was reported"; exit 1; }
  grep -q '^ui-words: ' <<<"$c_hits" || { echo "SELF-TEST FAILED: a UI word in a C crate was not caught"; exit 1; }
  grep -q 'lib.rs:.*env-var\|^env-var: .*lib.rs' <<<"$a_hits" || { echo "SELF-TEST FAILED: an env read in an A library was not caught"; exit 1; }
  grep -q 'main.rs' <<<"$a_hits" && { echo "SELF-TEST FAILED: an env read in main.rs was reported"; exit 1; }
  echo "self-test ok: P bans fire, C allows IO but not UI words, A libraries read no env outside main.rs"
  exit 0
fi

fail=0
long_files=0
[ "${1:-}" = "--long-files" ] && long_files=1

members=$(sed -n '/^members = \[/,/^\]/s#^  "crates/\(.*\)",$#\1#p' Cargo.toml)

# Every workspace member has a row in LAYERS, and every row names a crate, so a new crate cannot
# slip in unchecked. Every member also has a row in RULES.
layer_rows=$(printf '%s\n' "${LAYERS[@]}" | grep -v '^#')
for crate in $members; do
  grep -q "^$crate: [PCA]\$" <<<"$layer_rows" || { echo "ERROR: $crate has no row in LAYERS"; fail=1; }
  printf '%s\n' "${RULES[@]}" | grep -q "^$crate:" || { echo "ERROR: $crate has no row in RULES"; fail=1; }
done
for row in "${LAYERS[@]}"; do
  crate="${row%%:*}"
  [ -f "crates/$crate/Cargo.toml" ] || { echo "ERROR: LAYERS names $crate, which has no crates/$crate/Cargo.toml"; fail=1; }
done

# Dependencies point down only: a crate's DIRECT normal and build path dependencies (all
# features) are of its own tier or a lower one. Dev dependencies are outside it.
for row in "${LAYERS[@]}"; do
  crate="${row%%:*}"
  tier=$(tier_of "$crate")
  deps=$(cargo tree -p "$crate" --depth 1 -e normal,build --prefix none --all-features 2>/dev/null \
    | grep -E '\((/|https://github.com/PoHsuanLai/)' | awk '{print $1}' | grep -vx "$crate" | sort -u)
  bad=0
  for dep in $deps; do
    dep_tier=$(tier_of "$dep")
    # A dependency from another repo (stoker, porter, docket, almanac) has no row here.
    [ -z "$dep_tier" ] && continue
    if [ "$(rank "$dep_tier")" -gt "$(rank "$tier")" ]; then
      echo "LAYER: $crate ($tier) depends on $dep ($dep_tier), which is above it"; bad=1; fail=1
    fi
  done
  [ "$bad" -eq 0 ] && echo "layers hold: $crate ($tier) depends only on its own tier or below"
done

# ONLY: crates whose DIRECT normal and build path dependencies are exactly this list. A dependency
# not listed is a leak; so is one the crate no longer has, so the table stays exact.
# cua-bus carries serde bodies, but their fields use the vocabulary of the crates it names:
# almanac-core (EventRef in the log record), docket-core (Value, EffectBasis and the message
# drafts of a report), porter-core and porter-infer (counts, money, step and served-by ids) and
# prov (labels). None of them is a capability crate.
ONLY=(
  "cua-bus: almanac-core cua-action cua-desktop cua-types docket-core porter-core porter-infer prov"
)
for entry in "${ONLY[@]}"; do
  crate="${entry%%:*}"
  read -r -a allowed <<<"${entry#*:}"
  found=$(cargo tree -p "$crate" --depth 1 -e normal,build --prefix none --all-features 2>/dev/null \
    | grep -E '\((/|https://github.com/PoHsuanLai/)' | awk '{print $1}' | grep -vx "$crate" | sort -u | tr '\n' ' ')
  want=$(printf '%s\n' "${allowed[@]}" | sort -u | tr '\n' ' ')
  if [ "$found" != "$want" ]; then
    echo "ONLY: $crate depends on [${found% }], the table allows [${want% }]"
    fail=1
  else
    echo "only holds: $crate depends on [${found% }]"
  fi
done

# Source bans, with the KNOWN list.
for row in "${LAYERS[@]}"; do
  crate="${row%%:*}"
  hits=$(source_violations . "$crate")
  seen=""
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    ban="${hit%%: *}"
    lane=$(known_lane "$crate" "$ban")
    if [ -n "$lane" ]; then
      echo "KNOWN: $crate ($ban) is removed by lane $lane: ${hit#*: }"
      seen="$seen $ban"
    else
      echo "BAN: $crate ($(tier_of "$crate")): $hit"
      fail=1
    fi
  done <<<"$hits"
  for k in "${KNOWN[@]}"; do
    [ "${k%%:*}" = "$crate" ] || continue
    k_ban=$(cut -d: -f2 <<<"$k" | tr -d ' ')
    grep -qw -- "$k_ban" <<<"$seen" || { echo "STALE: KNOWN names $k_ban in $crate, which no longer breaks it; delete the row"; fail=1; }
  done
  [ -z "$hits" ] && echo "bans hold: $crate ($(tier_of "$crate")) source is clean"
done

# Source files over 500 lines: reported on request, not a failure yet.
if [ "$long_files" -eq 1 ]; then
  find crates -name '*.rs' -path '*/src/*' -not -path '*/target/*' -print0 \
    | xargs -0 wc -l | awk '$1 > 500 && $2 != "total" {print "LONG: " $2 " has " $1 " lines"}'
fi

for rule in "${RULES[@]}"; do
  crate="${rule%%:*}"
  read -r -a forbidden <<<"${rule#*:}"
  # A crate that cargo cannot find would make every check below pass vacuously.
  if ! cargo tree -p "$crate" --depth 0 >/dev/null 2>&1; then
    echo "ERROR: cargo tree cannot resolve $crate; the boundary was not checked"
    fail=1
    continue
  fi
  leaked=0
  for dep in "${forbidden[@]}"; do
    if cargo tree -p "$crate" -i "$dep" -e normal,build 2>/dev/null | grep -q .; then
      echo "LEAK: $crate depends on $dep"
      cargo tree -p "$crate" -i "$dep" -e normal,build 2>/dev/null | head -20
      leaked=1
      fail=1
    fi
  done
  if [ "$leaked" -eq 0 ]; then
    echo "boundary holds: $crate reaches none of ${forbidden[*]}"
  fi
done

# cua sits above docket and below sill in the repo order (stoker, porter, almanac, docket, cua,
# sill), and the GPL compositors are separate processes: nothing of sill's, and
# nothing of theirs (smithay itself), may enter any tree of this workspace,
# dev-dependencies included.
THEIRS="smithay"
workspace_tree=$(cargo tree --workspace -e normal,build,dev --prefix none --all-features 2>/dev/null | awk '{print $1}' | sort -u)
theirs_found=0
for name in $THEIRS; do
  if printf '%s\n' "$workspace_tree" | grep -qx "$name"; then
    echo "LEAK: the workspace depends on $name (the GPL fork)"
    theirs_found=1
    fail=1
  fi
done
if printf '%s\n' "$workspace_tree" | grep -q '^sill-'; then
  echo "LEAK: the workspace depends on a sill crate, which sits above cua"
  theirs_found=1
  fail=1
fi
if [ "$theirs_found" -eq 0 ]; then
  echo "boundary holds: no sill crate and nothing of the compositor enters the workspace"
fi

# cuad's `fixture-callers` is a security switch: with it on, cuad takes who is calling from a
# process tree and callers file the harness names. Built as the installer builds it (`-p` each
# installed package, no dev-dependencies), no shipped package turns it on.
for pkg in cuad; do
  # Each package's enabled features (`{f}`): `-e features` elides the edge a package's own feature makes.
  if cargo tree -p "$pkg" -e normal,build --prefix none --format '{p} {f}' 2>/dev/null \
    | grep -qE 'fixture-callers|test-proc-root'; then
    echo "TEST SWITCH: $pkg's build turns on the fixture tree"
    fail=1
  else
    echo "fixture tree off in $pkg's build"
  fi
done

# No shipped source names the harness's variables, apart from the two constants, each behind
# `#[cfg(feature = "fixture-callers")]` on the line before.
leaks=$(grep -rnE 'CUAD_TEST_|PROC_ROOT' crates --include='*.rs' --exclude-dir=tests \
  | grep -vE '^crates/cuad/src/callers.rs:[0-9]+:pub const (SYSTEM_CALLERS_VAR|PROC_ROOT_VAR): &str = "CUAD_TEST_(SYSTEM_CALLERS|PROC_ROOT)";$' \
  | grep -vE '^crates/cuad/src/callers.rs:[0-9]+:.*env\(PROC_ROOT_VAR\)' || true)
for line in $(grep -nE '^pub const (SYSTEM_CALLERS_VAR|PROC_ROOT_VAR)' crates/cuad/src/callers.rs | cut -d: -f1); do
  sed -n "$((line - 1))p" crates/cuad/src/callers.rs | grep -qx '#\[cfg(feature = "fixture-callers")\]' \
    || leaks="$leaks"$'\n'"crates/cuad/src/callers.rs:$line: not behind the fixture-callers cfg"
done
if [ -n "${leaks//$'\n'/}" ]; then
  echo "TEST SWITCH: a shipped crate names a harness switch:"
  echo "$leaks"
  fail=1
else
  echo "no shipped crate names a harness switch outside its cfg-gated constants"
fi

exit "$fail"
