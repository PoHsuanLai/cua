#!/usr/bin/env bash
# One cloud eval run: a hosted model reached through porter's inferd, with the API key held by
# accountd. Everything runs on a private bus in scratch directories; the real session bus, the real
# home and the real Secret Service are never touched, and the eval process gets an environment with
# no key in it.
#
#   scripts/cloud-eval.sh --accountd BIN --inferd BIN --eval BIN --home DIR --entry FILE --label NAME \
#       [--catalog DIR] [--tier fast|balanced|best] [--cents N] [-- extra cua-eval-cloud flags]
#
# --accountd  porter's accountd built with `--features test-proc-root,test-keys`.
# --inferd    porter's inferd built with `--features test-proc-root`, at the porter rev cua pins.
# --eval      the cua-eval-cloud binary.
# --home      the filled accountd home (docket's dev/live/cloud-key.sh): `accountd.keys` (0600) is only
#             pointed at by accountd's ACCOUNTD_KEYS, never read by this script or the eval.
# --entry     the trial catalogue entry of the model (eval/cloud-catalog/<id>.toml).
# --catalog   the directory whose entries inferd is given (default eval/cloud-catalog).
# --cents     inferd's [ai.spend] daily cap on the account and on the app (default 300).
# The eval runs as the intentd unit of inferd's caller table (the account's grant names org.quire.Intents).
set -euo pipefail

die() { echo "cloud-eval: $*" >&2; exit 2; }
here=$(cd "$(dirname "$0")/.." && pwd -P)
accountd=; inferd=; evalbin=; home=; entry=; label=; catalog=$here/eval/cloud-catalog; tier=balanced; cents=300
while [ $# -gt 0 ]; do
  case "$1" in
    --accountd) accountd=$2; shift 2 ;;
    --inferd) inferd=$2; shift 2 ;;
    --eval) evalbin=$2; shift 2 ;;
    --home) home=$2; shift 2 ;;
    --entry) entry=$2; shift 2 ;;
    --label) label=$2; shift 2 ;;
    --catalog) catalog=$2; shift 2 ;;
    --tier) tier=$2; shift 2 ;;
    --cents) cents=$2; shift 2 ;;
    --) shift; break ;;
    *) die "unknown option $1" ;;
  esac
done
for v in accountd inferd evalbin home entry label; do [ -n "${!v}" ] || die "--${v/evalbin/eval} is required"; done
for f in "$accountd" "$inferd" "$evalbin"; do [ -x "$f" ] || die "$f is not an executable"; done
[ -f "$entry" ] || die "$entry is not a file"
case "$home" in /*) ;; *) die "--home must be absolute" ;; esac
[ -d "$home" ] || die "$home is not a directory"
[ -z "$(find "$home" -maxdepth 0 -perm /077)" ] || die "$home has group or other permission bits"
[ -z "$(find "$home/accountd.keys" -maxdepth 0 -perm /077)" ] || die "the key file has group or other permission bits"
id=$(sed -n 's/^id = "\(.*\)"/\1/p' "$entry" | head -1)
[ -n "$id" ] || die "no id in $entry"
entry=$(cd "$(dirname "$entry")" && pwd -P)/$(basename "$entry")

# The world: short paths, because a bus socket path is limited.
world=$(mktemp -d /tmp/cw.XXXXXX)
chmod 700 "$world"
mkdir -p "$world"/{logs,data/stoker/catalog,config/quire,config/porter,cache,run,proc/units,none}
chmod 700 "$world/run"
cp "$catalog"/*.toml "$world/data/stoker/catalog/"
# accountd's non-secret records (the registry and the provider file) are copied in; the key file is not.
[ -d "$home/state" ] && { mkdir -p "$world/.local/state"; cp -r "$home/state/." "$world/.local/state/"; }
[ -d "$home/data" ] && cp -r "$home/data/." "$world/data/"
cat > "$world/config/porter/callers.toml" <<'TOML'
[[caller]]
app = "org.quire.Inference"
unit = "inferd.service"
role = "porter_daemon"
TOML
cat > "$world/config/quire/inferd.toml" <<TOML
[ai]
local_only = "off"

[ai.floor]
screen = "anywhere"

[ai.model.text]
fast = "cloud/$id"
balanced = "cloud/$id"
best = "cloud/$id"

[ai.spend]
account_daily_cents = $cents
app_daily_cents = $cents

[callers.apps]
"org.quire.Intents" = ["intentd.service"]
TOML

slice=0::/user.slice/user-1000.slice/user@1000.service/app.slice
place() { # pid unit
  mkdir -p "$WORLD/proc/$1"
  printf '%s/%s.service\n' "$slice" "$2" > "$WORLD/proc/$1/cgroup"
  printf '%s' "$1" > "$WORLD/proc/units/$2.service"
}
world_env=(HOME="$world" PATH=/usr/bin:/bin XDG_DATA_HOME="$world/data" XDG_DATA_DIRS="$world/none"
  XDG_CONFIG_HOME="$world/config" XDG_CONFIG_DIRS="$world/none" XDG_CACHE_HOME="$world/cache"
  XDG_RUNTIME_DIR="$world/run")
ledger=$world/.local/state/quire/inferd/spend.json

# Everything below `inner` runs inside dbus-run-session, so it is written to a script.
cleanup() {
  for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
wait_name() { # bus-address name
  for _ in $(seq 1 200); do
    busctl --address="$1" status "$2" >/dev/null 2>&1 && return 0
    sleep 0.3
  done
  echo "cloud-eval: $2 never appeared on the bus; see $WORLD/logs" >&2
  return 1
}
inner() {
  local addr=$DBUS_SESSION_BUS_ADDRESS
  # accountd: the one process with the key file named.
  ( place "$BASHPID" accountd
    exec env -i "${world_env[@]}" DBUS_SESSION_BUS_ADDRESS="$addr" DBUS_SYSTEM_BUS_ADDRESS="$addr" \
      ACCOUNTD_KEYS="file:$HOMEDIR/accountd.keys" ACCOUNTD_PROC_ROOT="$WORLD/proc" \
      "$ACCOUNTD" >"$WORLD/logs/accountd.log" 2>&1 ) &
  pids+=($!)
  wait_name "$addr" org.quire.Accounts1
  ( place "$BASHPID" inferd
    exec env -i "${world_env[@]}" DBUS_SESSION_BUS_ADDRESS="$addr" DBUS_SYSTEM_BUS_ADDRESS="$addr" \
      INFERD_PROC_ROOT="$WORLD/proc" "$INFERD" >"$WORLD/logs/inferd.log" 2>&1 ) &
  pids+=($!)
  wait_name "$addr" org.quire.Inference1
  echo "cloud-eval: accountd and inferd are up on the private bus"
  # The eval: no key variable, no key file; placed as the intentd unit.
  ( place "$BASHPID" intentd
    exec env -i "${world_env[@]}" DBUS_SESSION_BUS_ADDRESS="$addr" \
      "$EVALBIN" --entry "$ENTRY" --label "$LABEL" --tier "$TIER" "$@" )
}
{
  echo '#!/usr/bin/env bash'
  echo 'set -euo pipefail'
  echo "slice='$slice'"
  echo "world_env=($(printf '%q ' "${world_env[@]}"))"
  declare -f place wait_name inner cleanup
  echo 'pids=(); trap cleanup EXIT; inner "$@"'
} > "$world/inner.sh"
chmod 700 "$world/inner.sh"
set +e
env -i "${world_env[@]}" WORLD="$world" ACCOUNTD="$accountd" INFERD="$inferd" EVALBIN="$evalbin" \
  HOMEDIR="$home" ENTRY="$entry" LABEL="$label" TIER="$tier" \
  dbus-run-session -- bash "$world/inner.sh" "$@"
status=$?
set -e
echo "cloud-eval: inferd's ledger:"
python3 -I - "$ledger" <<'PY' || true
import json, sys
try:
    doc = json.load(open(sys.argv[1]))
except Exception as e:
    print("  no ledger:", e); sys.exit(0)
for row in doc.get("rows", []):
    print("  ", row.get("period"), row.get("scope"), row.get("spent"))
PY
echo "cloud-eval: logs and scratch world kept at $world (logs/inferd.log, logs/accountd.log); remove it when done"
exit "$status"
