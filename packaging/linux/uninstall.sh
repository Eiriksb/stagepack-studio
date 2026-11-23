#!/bin/sh
# Stagepack Studio: takes away what install.sh put in place, and only that: each file is removed
# only if it is still ours (it carries our marker, or is the very icon we installed, or is the link
# to our program). Never touches the app's data folder (~/.local/share/stagepack-studio).
#
#   sh ~/.local/lib/stagepack-studio/uninstall.sh
set -eu

refuse() {
    printf 'uninstall.sh: %s Nothing was changed.\n' "$*" >&2
    exit 1
}
say() { printf '%s\n' "$*"; }

OWN=$(cd "$(dirname "$0")" && pwd -P)
[ "${OWN##*/}" = stagepack-studio ] && [ -f "$OWN/install-manifest.txt" ] && [ -f "$OWN/paths.sh" ] || refuse "This isn't an installed copy of Stagepack Studio (there is no install-manifest.txt beside it)."

# The manifest only holds the settings install used and the default it replaced; they are read as
# text (never run), and checked by the same rules as at install.
PREV_CONFIG=
PREV_APPS=
NEW_CONFIG=
NEW_APPS=
while IFS= read -r _l || [ -n "$_l" ]; do
    case $_l in
        DATA=*) XDG_DATA_HOME=${_l#*=} ;;
        CONFIG=*) XDG_CONFIG_HOME=${_l#*=} ;;
        BIN=*) XDG_BIN_HOME=${_l#*=} ;;
        prev_config=*) PREV_CONFIG=${_l#*=} ;;
        prev_apps=*) PREV_APPS=${_l#*=} ;;
        new_config=*) NEW_CONFIG=${_l#*=} ;;
        new_apps=*) NEW_APPS=${_l#*=} ;;
    esac
done <"$OWN/install-manifest.txt"
# shellcheck source=paths.sh
. "$OWN/paths.sh"
[ "$(cd "$APP" 2>/dev/null && pwd -P)" = "$OWN" ] || refuse "This folder isn't where the settings in its install-manifest.txt say Stagepack Studio was installed."

# Is a menu entry with this ID still to be found, in the user's or the system's folders?
entry_exists() {
    [ -f "$APPS/$1" ] && return 0
    _old_ifs=$IFS
    IFS=:
    for _d in ${XDG_DATA_DIRS:-/usr/local/share:/usr/share}; do
        [ -n "$_d" ] && [ -f "$_d/applications/$1" ] && { IFS=$_old_ifs; return 0; }
    done
    IFS=$_old_ifs
    return 1
}

# Our entry comes out of a mimeapps.list. In the default list the program it replaced goes back
# if that program is still installed. The file goes only if we made it and nothing else is in it.
fix_list() {
    _file=$1
    _prev=$2
    _made=$3
    [ -f "$_file" ] || return 0
    _tmp="$_file.stagepack-$$"
    _changed=0
    _sec=
    while IFS= read -r _line || [ -n "$_line" ]; do
        case $_line in
            "["*"]"*)
                _sec=$_line
                printf '%s\n' "$_line"
                ;;
            "$TYPE="*)
                _rest=
                _hit=0
                _old_ifs=$IFS
                IFS=';'
                for _v in ${_line#*=}; do
                    if [ "$_v" = "$ENTRY" ]; then _hit=1; elif [ -n "$_v" ]; then _rest="$_rest$_v;"; fi
                done
                if [ $_hit = 1 ]; then
                    _changed=1
                    _back=
                    if [ "$_sec" = "[Default Applications]" ]; then
                        for _v in $_prev; do
                            if entry_exists "$_v"; then _back="$_back$_v;"; fi
                        done
                    fi
                    IFS=$_old_ifs
                    _all="$_back$_rest"
                    [ -z "$_all" ] || printf '%s=%s\n' "$TYPE" "$_all"
                else
                    IFS=$_old_ifs
                    printf '%s\n' "$_line"
                fi
                IFS=$_old_ifs
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

# Removes a file only when it is ours; says so when it leaves one.
remove_marked() {
    if [ -f "$1" ] && [ ! -L "$1" ] && grep -qF "$MARK" "$1"; then rm -f "$1"; elif [ -e "$1" ] || [ -L "$1" ]; then say "Left $1, which is no longer Stagepack Studio's."; fi
}
remove_icon() {
    if [ -f "$1" ] && [ ! -L "$1" ] && cmp -s "$1" "$OWN/icons/icon-$2.png"; then rm -f "$1"; elif [ -e "$1" ] || [ -L "$1" ]; then say "Left $1, which is no longer Stagepack Studio's."; fi
}

main() {
    cd /
    say "Removing Stagepack Studio from $APP and the entries it added. Your data folder $DATA/stagepack-studio is not touched."
    set -f
    if [ -L "$BIN/stagepack-studio" ] && [ "$(readlink "$BIN/stagepack-studio")" = "$APP/stagepack-studio" ]; then
        rm -f "$BIN/stagepack-studio"
    elif [ -e "$BIN/stagepack-studio" ] || [ -L "$BIN/stagepack-studio" ]; then
        say "Left $BIN/stagepack-studio, which is not our link."
    fi
    remove_marked "$APPS/stagepack-studio.desktop"
    remove_marked "$MIMEDIR/packages/stagepack-studio.xml"
    set +f
    for _png in "$OWN"/icons/icon-*.png; do
        [ -e "$_png" ] || continue
        _n=${_png##*/icon-}
        _n=${_n%.png}
        remove_icon "$DATA/icons/hicolor/${_n}x${_n}/apps/stagepack-studio.png" "$_n"
        remove_icon "$DATA/icons/hicolor/${_n}x${_n}/mimetypes/application-vnd.stagepack.talk.png" "$_n"
    done
    set -f
    fix_list "$CONFIG/mimeapps.list" "$PREV_CONFIG" "$NEW_CONFIG"
    fix_list "$APPS/mimeapps.list" "$PREV_APPS" "$NEW_APPS"
    set +f

    # The shared databases are rebuilt without us.
    if [ -d "$MIMEDIR" ] && command -v update-mime-database >/dev/null 2>&1; then
        update-mime-database "$MIMEDIR" >/dev/null 2>&1 || true
    fi
    if [ -d "$APPS" ] && command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APPS" >/dev/null 2>&1 || true
    fi
    if [ -f "$DATA/icons/hicolor/index.theme" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
    fi

    # The program's own folder: the only folder ever removed.
    rm -rf "$APP"
    say "Done. Stagepack Studio is removed."
}

main
exit 0
