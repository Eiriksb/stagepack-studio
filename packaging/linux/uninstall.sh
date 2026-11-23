#!/bin/sh
# Stagepack Studio: takes away exactly what install.sh put in place, using the list it kept
# (install-manifest.txt, in the program's folder). Never touches the app's data folder
# (~/.local/share/stagepack-studio) or anything it didn't write.
#
#   sh ~/.local/lib/stagepack-studio/uninstall.sh
set -eu

refuse() {
    printf 'uninstall.sh: %s Nothing was changed.\n' "$*" >&2
    exit 1
}
say() { printf '%s\n' "$*"; }
trim() {
    _p=$1
    while :; do
        case $_p in
            */) _p=${_p%/} ;;
            *) break ;;
        esac
    done
    printf '%s' "$_p"
}
NL='
'
TYPE=application/vnd.stagepack.talk
ENTRY=stagepack-studio.desktop

[ -n "${HOME:-}" ] || refuse "HOME isn't set, so there is no home folder to work in."
HOME=$(trim "$HOME")
[ -n "$HOME" ] || refuse "HOME is the top of the disk, which is not a home folder."
case $HOME in /*) ;; *) refuse "HOME is a relative path, so there is no clear home folder." ;; esac
[ "$(id -u)" != 0 ] || refuse "This is for one user and must not be run as root."
for _var in XDG_DATA_HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_BIN_HOME; do
    eval "_val=\${$_var:-}"
    case $_val in
        '' | /*) ;;
        *) refuse "$_var is a relative path, and it has to be a full path." ;;
    esac
done
DATA=$(trim "${XDG_DATA_HOME:-$HOME/.local/share}")
CONFIG=$(trim "${XDG_CONFIG_HOME:-$HOME/.config}")
BIN=$(trim "${XDG_BIN_HOME:-$HOME/.local/bin}")
CACHE=$(trim "${XDG_CACHE_HOME:-$HOME/.cache}")
[ -n "$DATA" ] && [ -n "$CONFIG" ] && [ -n "$BIN" ] || refuse "One of the folders is the top of the disk."

APP=$(cd "$(dirname "$0")" && pwd -P)
[ "${APP##*/}" = stagepack-studio ] && [ -f "$APP/install-manifest.txt" ] || refuse "This isn't an installed copy of Stagepack Studio (there is no install-manifest.txt beside it)."
case "$APP/" in "$DATA/stagepack-studio/"*) refuse "This folder is inside the app's data folder, which is never touched." ;; esac

MANIFEST=$(cat "$APP/install-manifest.txt")

# Only paths under the user's own folders, written out in full, are ever touched.
inside_ours() {
    case $1 in
        *"/../"* | */.. | */. | "") return 1 ;;
    esac
    for _root in "$HOME" "$DATA" "$CONFIG" "$BIN" "$CACHE" "$(dirname "$APP")"; do
        case $1 in "$_root"/*) return 0 ;; esac
    done
    return 1
}
in_manifest() {
    case "$NL$MANIFEST$NL" in *"$NL$1$NL"*) return 0 ;; esac
    return 1
}

# Our entry comes out of a mimeapps.list; the file goes only if we made it and nothing else is in it.
strip_default() {
    _file=$1
    _made=$2
    [ -f "$_file" ] || return 0
    _tmp="$_file.stagepack-$$"
    _changed=0
    while IFS= read -r _line || [ -n "$_line" ]; do
        case $_line in
            "$TYPE="*)
                _rest=
                _hit=0
                _vals=${_line#*=}
                _old_ifs=$IFS
                IFS=';'
                for _v in $_vals; do
                    if [ "$_v" = "$ENTRY" ]; then _hit=1; elif [ -n "$_v" ]; then _rest="$_rest$_v;"; fi
                done
                IFS=$_old_ifs
                if [ $_hit = 1 ]; then
                    _changed=1
                    [ -z "$_rest" ] || printf '%s=%s\n' "$TYPE" "$_rest"
                else
                    printf '%s\n' "$_line"
                fi
                ;;
            *) printf '%s\n' "$_line" ;;
        esac
    done <"$_file" >"$_tmp"
    if [ $_changed = 1 ]; then cat "$_tmp" >"$_file"; fi
    rm -f "$_tmp"
    if [ "$_made" = 1 ] && ! grep -qv -e '^[[:space:]]*$' -e '^\[.*\][[:space:]]*$' "$_file"; then
        rm -f "$_file"
    fi
}

main() {
    cd /
    say "Removing Stagepack Studio from $APP and the entries it added. Your data folder $DATA/stagepack-studio is not touched."
    _dirs=
    _trees=
    while IFS= read -r _entry; do
        _kind=${_entry%% *}
        _path=${_entry#? }
        [ -n "$_entry" ] || continue
        inside_ours "$_path" || { say "Skipped $_path (not a place install.sh writes)."; continue; }
        case $_kind in
            f)
                if [ -L "$_path" ]; then
                    case $(readlink "$_path") in
                        "$APP"/*) rm -f "$_path" ;;
                        *) say "Left $_path, which is a link to something else." ;;
                    esac
                elif [ -f "$_path" ]; then
                    rm -f "$_path"
                fi
                ;;
            d) _dirs="$_path${_dirs:+$NL$_dirs}" ;;
            t) _trees="$_path${_trees:+$NL$_trees}" ;;
            a) strip_default "$_path" 0 ;;
            A) strip_default "$_path" 1 ;;
        esac
    done <<LIST
$MANIFEST
LIST

    # The shared databases: rebuilt without us, unless the whole thing was ours to start with.
    if [ -d "$DATA/mime" ]; then
        if [ -n "$_trees" ] && [ -z "$(ls -A "$DATA/mime/packages" 2>/dev/null)" ]; then
            case $_trees in *"$DATA/mime"*) rm -rf "$DATA/mime" ;; esac
        elif command -v update-mime-database >/dev/null 2>&1; then
            update-mime-database "$DATA/mime" >/dev/null 2>&1 || true
        fi
    fi
    if [ -d "$DATA/applications" ] && ! in_manifest "f $DATA/applications/mimeinfo.cache" && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
    fi
    if [ -f "$DATA/icons/hicolor/icon-theme.cache" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
    fi

    # The program's own folder, then the empty folders install.sh made, innermost first.
    rm -rf "$APP"
    if [ -n "$_dirs" ]; then
        while IFS= read -r _d; do
            rmdir "$_d" 2>/dev/null || true
        done <<LIST
$_dirs
LIST
    fi
    say "Done. Stagepack Studio is removed."
}

main
exit 0
