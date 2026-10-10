#!/usr/bin/env bash
# Crate boundaries, mechanically enforced (ARCHITECTURE.md section 1 is the table; this is its
# mechanical form).
#
# `cargo tree -i <dep>` exits 101 when the dependency is absent, which is precisely the state
# we want. Checking the exit status would therefore fail whenever the boundary holds, so we
# check for OUTPUT instead: any line naming the dependency is a leak.
set -uo pipefail
cd "$(dirname "$0")/.."

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
  # The bus bodies: serde only. The capability crates (accessibility, input, capture) are not
  # reachable, so a consumer that reads the bodies (sill, the D-Bus codec) links none of them;
  # the leaf types they share live in cua-types.
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
  # (EDGES below), and docket-client's `InProcess` (which links docket-router, hence cedar) is
  # behind its `in_process` feature, which only cuad's tests turn on (a dev-dependency).
  "cuad: reqwest hyper hyper-util rustls pipewire ort fastembed rmcp cedar-policy"
)
fail=0

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
# nothing of theirs (smithay, cosmic-comp itself), may enter any tree of this workspace,
# dev-dependencies included.
THEIRS="cosmic-comp smithay cosmic-comp-config"
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
  echo "boundary holds: no sill crate and nothing of the cosmic-comp fork enters the workspace"
fi

# The allowed edges between our own crates: each crate's DIRECT normal and build path
# dependencies (all features), and nothing else. A dependency not listed is a leak; so is one
# the crate no longer has, so the table stays exact. Dev dependencies are outside it.
EDGES=(
  "cua-desktop: casement-protocols cua-action docket-core porter-core prov"
  "maskcap: casement-protocols cua-action cua-desktop cua-types"
  "seat-input: cua-action cua-desktop cua-types porter-core"
  "a11y-tree: cua-action cua-desktop cua-types porter-core prov"
  "cua-types: cua-desktop porter-core"
  "cua-bus: almanac-core cua-action cua-desktop cua-types docket-core porter-core porter-infer prov"
  "cua-run: a11y-tree cua-action cua-bus cua-desktop docket-core maskcap porter-core porter-infer prov seat-input"
  "cua-host: a11y-tree almanac-client almanac-core cua-bus cua-desktop cua-run docket-client docket-core maskcap porter-core prov"
  "cua-dbus: cua-bus prov"
  "cua-fake: a11y-tree cua-action cua-bus cua-desktop cua-parse cua-run docket-core maskcap model-provider model-replay porter-core porter-infer prov seat-input"
  "cua-eval: a11y-tree cua-action cua-bus cua-desktop cua-fake cua-parse cua-run cua-session cua-vendors docket-core maskcap model-catalog model-http model-openai-compat model-provider porter-core porter-infer prov seat-input vision-prep"
  "cua-eval-cloud: cua-eval model-catalog model-provider porter-client porter-core porter-infer vision-prep"
  "cuad: a11y-tree almanac-client almanac-core cua-bus cua-dbus cua-desktop cua-host cua-run docket-client docket-core maskcap porter-client porter-core porter-infer prov seat-input"
)
for edge in "${EDGES[@]}"; do
  crate="${edge%%:*}"
  read -r -a allowed <<<"${edge#*:}"
  found=$(cargo tree -p "$crate" --depth 1 -e normal,build --prefix none --all-features 2>/dev/null \
    | grep -E '\((/|https://github.com/PoHsuanLai/)' | awk '{print $1}' | grep -vx "$crate" | sort -u | tr '\n' ' ')
  want=$(printf '%s\n' "${allowed[@]}" | grep . | sort -u | tr '\n' ' ')
  if [ "$found" != "$want" ]; then
    echo "EDGE: $crate depends on [${found% }], the table allows [${want% }]"
    fail=1
  else
    echo "edges hold: $crate depends on [${found% }]"
  fi
done

# Every workspace member has a row above, so a new crate cannot slip in unchecked.
for member in $(sed -n 's#^  "crates/\(.*\)",$#\1#p' Cargo.toml); do
  printf '%s\n' "${EDGES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in EDGES"; fail=1; }
  printf '%s\n' "${RULES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in RULES"; fail=1; }
done

exit "$fail"
