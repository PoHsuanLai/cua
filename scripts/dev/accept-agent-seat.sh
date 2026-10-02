#!/usr/bin/env bash
# Acceptance for the fork's agent seat (cua.md Spike C, fork changes F1 and F2). DEV ONLY, by
# hand, never CI. A stub until the fork's fill 4 builds the seat: it checks its preconditions
# and exits 2.
#
# What it will prove, against the quire fork of cosmic-comp (branch quire, `src/quire/**`):
#   1. The agent clicks in window A while the physical pointer hovers window B: the person's
#      cursor does not move and B keeps keyboard focus.
#   2. Menus, popups and combo boxes work for at least 80% of the matrix in GTK4, Qt6, Firefox
#      and Chromium (open a menu and select, combo box, slider drag, drag and drop, text entry,
#      scroll).
#   3. 0 agent events reach a layer-shell, session-lock or consent surface (an overlay layer
#      surface under the agent's pointer receives no enter, motion, click or key).
#   4. An injected `ctrl+alt+space` triggers none of the compositor's shortcuts.
#   5. The kill chord (hardware) revokes every lease: `Cua1.Manager.StopAll` runs follow.
# Below 80% on item 2 the fork's mode falls back to "focus borrow" (EI on the user's seat,
# gated to the lease, physical input pausing the run), per cua.md.
#
# Safety, as shell-host's `accept-lock.sh` does it: the compositor runs nested under a throwaway
# parent with a private XDG_RUNTIME_DIR, a private session bus and scratch HOME and XDG dirs.
# The person's `wayland-0` and their bus are never touched, and nothing here reads or writes
# their real data.
#
# Inputs:
#   COSMIC_COMP   the fork's binary, e.g. ~/cosmic-comp/target/release/cosmic-comp
#   SHELL_HOST_DIR  shell-host's checkout (default ~/shell-host), for scripts/dev/nested.sh and
#                   scripts/dev/kwin-virtual.sh
# Output: scripts/dev/out/accept-agent-seat.json (one object per criterion).
set -euo pipefail

: "${COSMIC_COMP:?set COSMIC_COMP to the quire fork cosmic-comp binary}"
test -x "$COSMIC_COMP" || { echo "accept-agent-seat: $COSMIC_COMP is not executable" >&2; exit 2; }
SHELL_HOST_DIR="${SHELL_HOST_DIR:-$HOME/shell-host}"
test -d "$SHELL_HOST_DIR/scripts/dev" || { echo "accept-agent-seat: no harness at $SHELL_HOST_DIR/scripts/dev" >&2; exit 2; }

echo "accept-agent-seat: not implemented (closes with the fork's fill 4: agent seat, InputOrigin gate, kill chord)" >&2
exit 2
