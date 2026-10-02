#!/usr/bin/env bash
# Acceptance for masked capture (cua.md Spike D, fork change F3, and cuad's own masking on stock).
# DEV ONLY, by hand, never CI. A stub until the fork's fill 4 builds the masked source: it checks
# its preconditions and exits 2.
#
# What it will prove, nested under a throwaway parent compositor:
#   1. Which toolkits declare `content_purpose = password` through text-input-v3 (GTK4 entry,
#      Qt6, Firefox, Chromium) and which expose role PasswordText over AT-SPI. Pass: the mask
#      fully covers the field in 100% of the declared cases (every pixel of the field's device
#      rectangle is black in the captured buffer, at scale 1.0 and 1.5).
#   2. The undeclared cases are recorded, not hidden: they are the honest limit of masking.
#   3. Stock: `maskcap::apply_masks` in cuad gives the same result from the unmasked toplevel
#      capture (`MaskReport.applied_by = Cuad`); fork: the buffer arrives masked
#      (`applied_by = Compositor`). In neither does an unmasked frame leave cuad.
#   4. A frame carries no pixel of another Space's content or of an overlay outside the lease.
#
# Safety: as accept-agent-seat.sh: private runtime dir, private bus, scratch HOME; the person's
# session is never touched.
#
# Inputs:
#   COSMIC_COMP   the fork's binary, e.g. ~/cosmic-comp/target/release/cosmic-comp
#                 (for the stock leg, the stock binary)
#   SHELL_HOST_DIR  shell-host's checkout (default ~/shell-host)
# Output: scripts/dev/out/accept-masking.json (one object per toolkit).
set -euo pipefail

: "${COSMIC_COMP:?set COSMIC_COMP to the cosmic-comp binary under test}"
test -x "$COSMIC_COMP" || { echo "accept-masking: $COSMIC_COMP is not executable" >&2; exit 2; }
SHELL_HOST_DIR="${SHELL_HOST_DIR:-$HOME/shell-host}"
test -d "$SHELL_HOST_DIR/scripts/dev" || { echo "accept-masking: no harness at $SHELL_HOST_DIR/scripts/dev" >&2; exit 2; }

echo "accept-masking: not implemented (closes with the fork's fill 4: masked capture source)" >&2
exit 2
