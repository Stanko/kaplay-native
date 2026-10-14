#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(dirname "$script_dir")"

app_id=480
demo=0
build=0
use_debug=0
check_only=0
lib=""
bin=""
bundle=""
x11_args=()
host_args=()

usage() {
    cat <<'EOF'
usage: scripts/steam-overlay-test.sh [options] [-- <host args>...]

Sets up a direct-launch Steam overlay test: writes steam_appid.txt, locates
libsteam_api.so, preloads gameoverlayrenderer.so, then runs the host. Extra
arguments after -- replace the default bridge example and go to the host.

options:
  --build         build the host before launching
  --debug         use the debug binary instead of the release binary
  --check         validate the setup and exit without launching
  --demo          run examples/steam instead of the bridge example; the KAPLAY
                  bundle comes from $KAPLAY_BUNDLE or a local kaplay checkout
  --appid <id>    app id for steam_appid.txt (default: 480, Spacewar)
  --lib <path>    libsteam_api.so to load
  --bin <path>    host binary to run
  -h, --help      show this help
EOF
}

die() {
    echo "steam-overlay-test: $*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --build) build=1; shift ;;
        --debug) use_debug=1; shift ;;
        --check) check_only=1; shift ;;
        --demo) demo=1; shift ;;
        --appid)
            [[ $# -ge 2 ]] || die "--appid needs a value"
            app_id="$2"
            shift 2
            ;;
        --lib)
            [[ $# -ge 2 ]] || die "--lib needs a value"
            lib="$2"
            shift 2
            ;;
        --bin)
            [[ $# -ge 2 ]] || die "--bin needs a value"
            bin="$2"
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        --) shift; host_args=("$@"); break ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
done

[[ "$app_id" =~ ^[0-9]+$ ]] || die "app id must be numeric, got '$app_id'"

pgrep -x steam >/dev/null 2>&1 ||
    die "the Steam client is not running; start it and sign in, then retry"

profile=release
[[ $use_debug -eq 1 ]] && profile=debug
[[ -n "$bin" ]] || bin="$repo_root/target/$profile/kaplay-native-host"

if [[ $build -eq 1 || ! -x "$bin" ]]; then
    echo "steam-overlay-test: building $profile host"
    cargo build --profile "$profile" -p kaplay-native-host --manifest-path "$repo_root/Cargo.toml"
fi
[[ -x "$bin" ]] || die "host binary not found: $bin"

if [[ -z "$lib" ]]; then
    if [[ -n "${KAPLAY_NATIVE_STEAM_LIB:-}" ]]; then
        lib="$KAPLAY_NATIVE_STEAM_LIB"
    elif [[ -f "$(dirname "$bin")/libsteam_api.so" ]]; then
        lib="$(dirname "$bin")/libsteam_api.so"
    else
        shopt -s nullglob
        candidates=(
            "$HOME"/.local/share/Steam/steamapps/common/*/resources/steamworks_sdk/redistributable_bin/linux64/libsteam_api.so
            "$HOME"/.steam/steam/steamapps/common/*/resources/steamworks_sdk/redistributable_bin/linux64/libsteam_api.so
            "$HOME"/steamworks_sdk/redistributable_bin/linux64/libsteam_api.so
        )
        shopt -u nullglob
        [[ ${#candidates[@]} -gt 0 ]] && lib="${candidates[0]}"
    fi
fi
[[ -n "$lib" && -f "$lib" ]] ||
    die "libsteam_api.so not found; pass --lib or set KAPLAY_NATIVE_STEAM_LIB"

overlay="$HOME/.local/share/Steam/ubuntu12_64/gameoverlayrenderer.so"
[[ -f "$overlay" ]] || overlay="$HOME/.steam/steam/ubuntu12_64/gameoverlayrenderer.so"
[[ -f "$overlay" ]] ||
    die "gameoverlayrenderer.so not found; a Steam client install provides it"

appid_file="$repo_root/steam_appid.txt"
if [[ "$(cat "$appid_file" 2>/dev/null || true)" != "$app_id" ]]; then
    printf '%s\n' "$app_id" > "$appid_file"
fi

if [[ ${#host_args[@]} -eq 0 ]]; then
    if [[ "${XDG_SESSION_TYPE:-}" == "wayland" || -n "${WAYLAND_DISPLAY:-}" ]]; then
        x11_args=(--x11)
    fi
    if [[ $demo -eq 1 ]]; then
        bundle="${KAPLAY_BUNDLE:-}"
        if [[ -z "$bundle" ]]; then
            for candidate in \
                "$repo_root/kaplay.js" \
                "$HOME/Projects/kaplay/dist/kaplay.js" \
                "$HOME/Projects/kaplay/dist/kaplay.mjs"; do
                if [[ -f "$candidate" ]]; then
                    bundle="$candidate"
                    break
                fi
            done
        fi
        [[ -n "$bundle" && -f "$bundle" ]] ||
            die "KAPLAY bundle not found for --demo; set KAPLAY_BUNDLE=/path/to/kaplay.js"
        host_args=(
            --script "$repo_root/examples/steam/main.js"
            --dir "$repo_root/examples/steam"
            --preload "$bundle"
            "${x11_args[@]}"
        )
    else
        host_args=(
            --script "$repo_root/examples/bridge/main.js"
            --dir "$repo_root/examples/bridge"
            "${x11_args[@]}"
        )
    fi
fi

echo "steam-overlay-test: app id   $app_id"
echo "steam-overlay-test: appid    $appid_file"
echo "steam-overlay-test: steam    $lib"
echo "steam-overlay-test: overlay  $overlay"
echo "steam-overlay-test: host     $bin"
[[ -n "$bundle" ]] && echo "steam-overlay-test: bundle   $bundle"
[[ ${#x11_args[@]} -gt 0 ]] && echo "steam-overlay-test: backend  x11 (Wayland session)"

if [[ $check_only -eq 1 ]]; then
    echo "steam-overlay-test: setup ok"
    exit 0
fi

echo "steam-overlay-test: launching; press Shift+Tab once the window is up"
if [[ ${#x11_args[@]} -eq 0 && ( "${XDG_SESSION_TYPE:-}" == "wayland" || -n "${WAYLAND_DISPLAY:-}" ) ]]; then
    echo "steam-overlay-test: note: the overlay needs --x11 on Wayland"
fi

cd "$repo_root"
export KAPLAY_NATIVE_STEAM_LIB="$lib"
export LD_PRELOAD="$overlay${LD_PRELOAD:+:$LD_PRELOAD}"
export SteamAppId="$app_id" SteamGameId="$app_id"
export RUST_LOG="${RUST_LOG:-info}"
exec "$bin" "${host_args[@]}"
