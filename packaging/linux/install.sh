#!/bin/sh
# Stagepack Studio: installs for the current user only. No administrator rights, no internet.
#
#   sh install.sh
#
# The program goes to ~/.local/lib/stagepack-studio (beside the folder named by XDG_DATA_HOME when that is set),
# a link to it in ~/.local/bin, and the menu entry, the .sptx file type and the icons in their usual places
# under the data folder. The app's own data folder (~/.local/share/stagepack-studio) is never touched.
# Running it again upgrades. uninstall.sh (kept inside the installed folder) takes it all away again.
set -eu

refuse() {
    printf 'install.sh: %s Nothing was changed.\n' "$*" >&2
    exit 1
}
say() { printf '%s\n' "$*"; }

SRC=$(cd "$(dirname "$0")" && pwd -P)
[ -f "$SRC/paths.sh" ] || refuse "This folder isn't a complete Stagepack Studio package (paths.sh is missing); unpack the .tar.zst again."
# shellcheck source=paths.sh
. "$SRC/paths.sh"

# ---- The folder this script is in must be a whole staged app ----
for _part in stagepack-studio stagepack-terminal bundle/bin/rustc stagepack-studio.desktop.in stagepack-studio-mime.xml icons uninstall.sh; do
    [ -e "$SRC/$_part" ] || refuse "This folder isn't a complete Stagepack Studio package ($_part is missing); unpack the .tar.zst again."
done
ICONS=
for _png in "$SRC"/icons/icon-*.png; do
    [ -e "$_png" ] || continue
    _n=${_png##*/icon-}
    ICONS="$ICONS ${_n%.png}"
done
[ -n "$ICONS" ] || refuse "This folder has no icons in it; unpack the .tar.zst again."

# ---- Tidy what a cut-short earlier run of this script left beside the program ----
# Only folders this script makes: a name ending in .new-<digits> or .old-<digits>, holding one of our installs.
is_leftover() {
    case ${1##*-} in '' | *[!0-9]*) return 1 ;; esac
    [ -d "$1" ] && [ ! -L "$1" ] && [ -f "$1/install-manifest.txt" ] && [ -f "$1/stagepack-studio" ]
}
for _stale in "$APP".new-*; do
    if is_leftover "$_stale"; then rm -rf "$_stale"; fi
done
for _stale in "$APP".old-*; do
    is_leftover "$_stale" || continue
    if [ ! -e "$APP" ] && [ ! -L "$APP" ]; then mv "$_stale" "$APP"; else rm -rf "$_stale"; fi
done

# ---- What is already there: nothing that isn't ours is replaced, and nothing is written through a link ----
if [ -L "$APP" ]; then
    refuse "$APP is a link, and the program won't be put over a link."
elif [ -e "$APP" ] && [ ! -d "$APP" ]; then
    refuse "$APP is a file, not a folder, and is in the way."
elif [ -d "$APP" ] && { [ ! -f "$APP/install-manifest.txt" ] || [ ! -e "$APP/stagepack-studio" ]; }; then
    refuse "$APP exists and doesn't look like an earlier install of Stagepack Studio, so it is left alone."
fi
if [ -L "$BIN/stagepack-studio" ]; then
    [ "$(readlink "$BIN/stagepack-studio")" = "$APP/stagepack-studio" ] || refuse "$BIN/stagepack-studio is a link to something else."
elif [ -e "$BIN/stagepack-studio" ]; then
    refuse "$BIN/stagepack-studio already exists and isn't a link from an earlier install."
fi
# A file of ours (it carries the marker) may be replaced; a link, a folder or anyone else's file may not.
check_marked() {
    if [ -L "$1" ]; then
        refuse "$1 is a link, and nothing is written through a link."
    elif [ -e "$1" ] && { [ ! -f "$1" ] || ! grep -qF "$MARK" "$1"; }; then
        refuse "$1 already exists and isn't from Stagepack Studio."
    fi
}
check_icon() {
    if [ -L "$1" ]; then
        refuse "$1 is a link, and nothing is written through a link."
    elif [ -e "$1" ]; then
        [ -f "$1" ] || refuse "$1 already exists and isn't from Stagepack Studio."
        cmp -s "$1" "$SRC/icons/icon-$2.png" && return 0
        [ -f "$APP/icons/icon-$2.png" ] && cmp -s "$1" "$APP/icons/icon-$2.png" && return 0
        refuse "$1 already exists and isn't from Stagepack Studio."
    fi
}
# An upgrade must use the folders the first install used, or its entries would be left behind.
if [ -f "$APP/install-manifest.txt" ]; then
    while IFS= read -r _l || [ -n "$_l" ]; do
        case $_l in
            DATA=* | CONFIG=* | BIN=*)
                _k=${_l%%=*}
                eval "_now=\$$_k"
                [ "${_l#*=}" = "$_now" ] || refuse "The earlier install used $_k=${_l#*=} and this one would use $_now; run the earlier uninstall.sh first, or use the same XDG settings."
                ;;
        esac
    done <"$APP/install-manifest.txt"
fi
check_marked "$APPS/stagepack-studio.desktop"
check_marked "$MIMEDIR/packages/stagepack-studio.xml"
for _n in $ICONS; do
    check_icon "$DATA/icons/hicolor/${_n}x${_n}/apps/stagepack-studio.png" "$_n"
    check_icon "$DATA/icons/hicolor/${_n}x${_n}/mimetypes/application-vnd.stagepack.talk.png" "$_n"
done

# The Exec line: quoted when the path needs it, as the Desktop Entry specification says.
EXEC_PATH="$APP/stagepack-studio"
case $EXEC_PATH in
    *[\"\`\$\\%]*) refuse "The program's folder name has a quote, backslash, dollar, backtick or percent sign in it, which a menu entry can't carry." ;;
    *[!A-Za-z0-9_./+:,@=-]*) EXEC_QUOTED="\"$EXEC_PATH\"" ;;
    *) EXEC_QUOTED=$EXEC_PATH ;;
esac

say "Stagepack Studio will be installed for you only:"
say "  program         $APP"
say "  command         $BIN/stagepack-studio"
say "  menu entry      $APPS/stagepack-studio.desktop"
say "  file type       $MIMEDIR/packages/stagepack-studio.xml  (.sptx)"
say "  icons           $DATA/icons/hicolor/<size>/apps and mimetypes"
say "  default for .sptx files  set in $CONFIG/mimeapps.list"
say "Your data folder $DATA/stagepack-studio is not touched."

# ---- What the manifest keeps: the settings used, and the default this install replaces ----
OLD_MANIFEST=
[ -f "$APP/install-manifest.txt" ] && OLD_MANIFEST=$(cat "$APP/install-manifest.txt")
old_value() {
    while IFS= read -r _l || [ -n "$_l" ]; do
        case $_l in "$1="*) printf '%s' "${_l#*=}"; return 0 ;; esac
    done <<LIST
$OLD_MANIFEST
LIST
}
# The default for our type in a mimeapps.list's [Default Applications], other programs only.
default_in() {
    [ -f "$1" ] || return 0
    _sec=
    _v=
    while IFS= read -r _l || [ -n "$_l" ]; do
        case $_l in
            "["*"]"*) _sec=$_l ;;
            "$TYPE="*) [ "$_sec" = "[Default Applications]" ] && { _v=${_l#*=}; break; } ;;
        esac
    done <"$1"
    _out=
    set -f
    _old_ifs=$IFS
    IFS=';'
    for _t in $_v; do
        [ -z "$_t" ] || [ "$_t" = "$ENTRY" ] || _out="$_out$_t;"
    done
    IFS=$_old_ifs
    set +f
    printf '%s' "$_out"
}
PREV_CONFIG=$(old_value prev_config)
PREV_APPS=$(old_value prev_apps)
[ -n "$PREV_CONFIG" ] || PREV_CONFIG=$(default_in "$CONFIG/mimeapps.list")
[ -n "$PREV_APPS" ] || PREV_APPS=$(default_in "$APPS/mimeapps.list")
NEW_CONFIG=$(old_value new_config)
NEW_APPS=$(old_value new_apps)
[ -e "$CONFIG/mimeapps.list" ] || NEW_CONFIG=1
[ -e "$APPS/mimeapps.list" ] || NEW_APPS=1
MANIFEST="DATA=$DATA${NL}CONFIG=$CONFIG${NL}BIN=$BIN${NL}prev_config=$PREV_CONFIG${NL}prev_apps=$PREV_APPS${NL}new_config=$NEW_CONFIG${NL}new_apps=$NEW_APPS"

# ---- Put the program in place: copy beside it, then swap, so a failure leaves the old one ----
NEW="$APP.new-$$"
OLD="$APP.old-$$"
SWAPPED=0
cleanup() {
    _s=$?
    trap - EXIT INT TERM HUP
    if [ "$SWAPPED" = 0 ] && [ -d "$OLD" ] && [ ! -e "$APP" ]; then mv "$OLD" "$APP"; fi
    if [ "$SWAPPED" = 1 ]; then rm -rf "$OLD"; fi
    rm -rf "$NEW"
    if [ "$_s" -ne 0 ]; then
        printf 'install.sh: stopped before finishing. An earlier install, if there was one, is still in place; run uninstall.sh from %s to tidy up anything half done (an empty folder may be left behind).\n' "$APP" >&2
    fi
    exit "$_s"
}
trap cleanup EXIT
trap 'exit 1' INT TERM HUP
# A hook for the tests only (harmless otherwise): stop at a chosen step, as a signal would.
hook() { [ "${STAGEPACK_INSTALL_STOP_AT:-}" != "$1" ] || kill -TERM $$; }
mkdir -p "$(dirname "$APP")"
mkdir "$NEW"
cp -a "$SRC/." "$NEW/"
hook copy
printf '%s\n' "$MANIFEST" >"$NEW/install-manifest.txt"
if [ -d "$APP" ]; then
    mv "$APP" "$OLD"
    hook swap
    if ! mv "$NEW" "$APP"; then
        mv "$OLD" "$APP"
        printf 'install.sh: couldn'"'"'t move the new program into place; the old one is still there.\n' >&2
        exit 1
    fi
    SWAPPED=1
    rm -rf "$OLD"
else
    mv "$NEW" "$APP"
    SWAPPED=1
fi

# ---- The rest, all under the user's folders ----
mkdir -p "$BIN" "$APPS" "$MIMEDIR/packages"
ln -sf "$APP/stagepack-studio" "$BIN/stagepack-studio"

# Checked again just before writing: never written through a link made since the first check.
no_link() { [ ! -L "$1" ] || refuse "$1 has become a link, and nothing is written through a link."; }
no_link "$APPS/stagepack-studio.desktop"
rm -f "$APPS/stagepack-studio.desktop"
while IFS= read -r _line || [ -n "$_line" ]; do
    case $_line in
        *@EXEC@*) printf '%s%s%s\n' "${_line%%@EXEC@*}" "$EXEC_QUOTED" "${_line#*@EXEC@}" ;;
        *) printf '%s\n' "$_line" ;;
    esac
done <"$SRC/stagepack-studio.desktop.in" >"$APPS/stagepack-studio.desktop"

no_link "$MIMEDIR/packages/stagepack-studio.xml"
rm -f "$MIMEDIR/packages/stagepack-studio.xml"
cp "$SRC/stagepack-studio-mime.xml" "$MIMEDIR/packages/stagepack-studio.xml"
for _n in $ICONS; do
    mkdir -p "$DATA/icons/hicolor/${_n}x${_n}/apps" "$DATA/icons/hicolor/${_n}x${_n}/mimetypes"
    for _t in apps/stagepack-studio.png mimetypes/application-vnd.stagepack.talk.png; do
        no_link "$DATA/icons/hicolor/${_n}x${_n}/$_t"
        rm -f "$DATA/icons/hicolor/${_n}x${_n}/$_t"
        cp "$SRC/icons/icon-$_n.png" "$DATA/icons/hicolor/${_n}x${_n}/$_t"
    done
done

if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database "$MIMEDIR" || say "Note: update-mime-database didn't finish, so .sptx files may not be recognised until you log in again."
else
    say "Note: update-mime-database isn't installed, so .sptx files may not be recognised."
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APPS" || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -f "$DATA/icons/hicolor/index.theme" ]; then
    gtk-update-icon-cache -f -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
fi
if command -v xdg-mime >/dev/null 2>&1; then
    xdg-mime default stagepack-studio.desktop application/vnd.stagepack.talk || say "Note: couldn't set Stagepack Studio as the program for .sptx files; use Open with on a .sptx file."
else
    say "Note: xdg-mime isn't installed, so use Open with to choose Stagepack Studio for .sptx files."
fi

say "Done. Stagepack Studio is installed."
case ":${PATH:-}:" in
    *":$BIN:"*) ;;
    *) say "Note: $BIN isn't on your PATH; add it to run 'stagepack-studio' from a terminal. The menu entry works without it." ;;
esac
say "To remove it: sh \"$APP/uninstall.sh\""
