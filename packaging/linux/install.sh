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

# Strips trailing slashes ("/" becomes empty).
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

# ---- Who and where: refuse anything that isn't a normal user's home ----
[ -n "${HOME:-}" ] || refuse "HOME isn't set, so there is no home folder to install into."
HOME=$(trim "$HOME")
[ -n "$HOME" ] || refuse "HOME is the top of the disk, which is not a home folder to install into."
case $HOME in /*) ;; *) refuse "HOME is a relative path, so there is no clear home folder to install into." ;; esac
[ "$(id -u)" != 0 ] || refuse "This installs for one user and must not be run as root; run it as the user who will use the program."
for _var in XDG_DATA_HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_BIN_HOME; do
    eval "_val=\${$_var:-}"
    case $_val in
        '' | /*) ;;
        *) refuse "$_var is a relative path, and it has to be a full path." ;;
    esac
done

if [ -n "${XDG_DATA_HOME:-}" ]; then
    DATA=$(trim "$XDG_DATA_HOME")
    [ -n "$DATA" ] || refuse "XDG_DATA_HOME is the top of the disk, which is not a data folder to install into."
    _up=$(dirname "$DATA")
    [ "$_up" != / ] || refuse "XDG_DATA_HOME is too close to the top of the disk to put the program beside it."
    APP="$_up/lib/stagepack-studio"
else
    DATA="$HOME/.local/share"
    APP="$HOME/.local/lib/stagepack-studio"
fi
CONFIG=$(trim "${XDG_CONFIG_HOME:-$HOME/.config}")
BIN=$(trim "${XDG_BIN_HOME:-$HOME/.local/bin}")
[ -n "$CONFIG" ] && [ -n "$BIN" ] || refuse "A folder named for the install is the top of the disk."
for _p in "$HOME" "$DATA" "$CONFIG" "$BIN" "$APP"; do
    case $_p in *"$NL"*) refuse "A folder name has a line break in it." ;; esac
done
case "$APP/" in "$DATA/stagepack-studio/"*) refuse "The program would land inside the app's data folder $DATA/stagepack-studio, which is never written to." ;; esac
case "$DATA/" in "$APP/"*) refuse "The data folder is inside where the program would go." ;; esac

# ---- The folder this script is in must be a whole staged app ----
SRC=$(cd "$(dirname "$0")" && pwd -P)
for _part in stagepack-studio stagepack-terminal bundle/bin/rustc; do
    [ -e "$SRC/$_part" ] || refuse "This folder isn't a complete Stagepack Studio package ($_part is missing); unpack the .tar.zst again."
done
for _part in stagepack-studio.desktop.in stagepack-studio-mime.xml icons uninstall.sh; do
    [ -e "$SRC/$_part" ] || refuse "This folder isn't a complete Stagepack Studio package ($_part is missing); unpack the .tar.zst again."
done

# ---- What is already there ----
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

# The Exec line: quoted when the path needs it, as the Desktop Entry specification says.
EXEC_PATH="$APP/stagepack-studio"
case $EXEC_PATH in
    *[\"\`\$\\%]*) refuse "The program's folder name has a quote, backslash, dollar, backtick or percent sign in it, which a menu entry can't carry." ;;
    *[!A-Za-z0-9_./+:,@=-]*) EXEC_QUOTED="\"$EXEC_PATH\"" ;;
    *) EXEC_QUOTED=$EXEC_PATH ;;
esac

APPS="$DATA/applications"
MIMEDIR="$DATA/mime"
ICONS=
for _png in "$SRC"/icons/icon-*.png; do
    [ -e "$_png" ] || continue
    _n=${_png##*/icon-}
    ICONS="$ICONS ${_n%.png}"
done
[ -n "$ICONS" ] || refuse "This folder has no icons in it; unpack the .tar.zst again."

say "Stagepack Studio will be installed for you only:"
say "  program         $APP"
say "  command         $BIN/stagepack-studio"
say "  menu entry      $APPS/stagepack-studio.desktop"
say "  file type       $MIMEDIR/packages/stagepack-studio.xml  (.sptx)"
say "  icons           $DATA/icons/hicolor/<size>/apps and mimetypes"
say "  default for .sptx files  set in $CONFIG/mimeapps.list"
say "Your data folder $DATA/stagepack-studio is not touched."

# ---- The manifest: what uninstall.sh will take away (paths outside the program's folder) ----
OLD_MANIFEST=
[ -f "$APP/install-manifest.txt" ] && OLD_MANIFEST=$(cat "$APP/install-manifest.txt")
MANIFEST=$OLD_MANIFEST
add() {
    case "$NL$MANIFEST$NL" in *"$NL$1$NL"*) return 0 ;; esac
    MANIFEST="${MANIFEST:+$MANIFEST$NL}$1"
}
# Notes the folders that don't exist yet and will be made for `$1`, outermost first.
note_dirs() {
    _list=
    _q=$1
    while [ ! -e "$_q" ] && [ "$_q" != / ] && [ -n "$_q" ]; do
        _list="$_q${_list:+$NL$_list}"
        _q=$(dirname "$_q")
    done
    [ -z "$_list" ] || while IFS= read -r _d; do add "d $_d"; done <<LIST
$_list
LIST
}
note_dirs "$(dirname "$APP")"
note_dirs "$BIN"
note_dirs "$APPS"
note_dirs "$DATA/icons"
note_dirs "$CONFIG"
[ -e "$MIMEDIR" ] || { note_dirs "$DATA"; add "t $MIMEDIR"; }
add "f $BIN/stagepack-studio"
add "f $APPS/stagepack-studio.desktop"
add "f $MIMEDIR/packages/stagepack-studio.xml"
for _n in $ICONS; do
    add "f $DATA/icons/hicolor/${_n}x${_n}/apps/stagepack-studio.png"
    add "f $DATA/icons/hicolor/${_n}x${_n}/mimetypes/application-vnd.stagepack.talk.png"
done
[ -e "$APPS/mimeinfo.cache" ] || add "f $APPS/mimeinfo.cache"
[ -e "$DATA/icons/hicolor/icon-theme.cache" ] || add "f $DATA/icons/hicolor/icon-theme.cache"
for _list in "$CONFIG/mimeapps.list" "$APPS/mimeapps.list"; do
    if [ -e "$_list" ]; then add "a $_list"; else add "A $_list"; fi
done
# Folders made for the icons (only listed when they're new).
for _n in $ICONS; do
    note_dirs "$DATA/icons/hicolor/${_n}x${_n}/apps"
    note_dirs "$DATA/icons/hicolor/${_n}x${_n}/mimetypes"
done

# ---- Put the program in place: copy beside it, then swap, so a failure leaves the old one ----
NEW="$APP.new-$$"
OLD="$APP.old-$$"
trap '_s=$?; rm -rf "$NEW"; [ "$_s" -eq 0 ] || printf "install.sh: stopped before finishing. Run uninstall.sh from %s to tidy up if anything was half done.\n" "$APP" >&2' EXIT
mkdir -p "$(dirname "$APP")"
rm -rf "$NEW" "$OLD"
mkdir "$NEW"
cp -a "$SRC/." "$NEW/"
printf '%s\n' "$MANIFEST" >"$NEW/install-manifest.txt"
if [ -d "$APP" ]; then
    mv "$APP" "$OLD"
    if ! mv "$NEW" "$APP"; then
        mv "$OLD" "$APP"
        refuse "Couldn't move the new program into place; the old one is still there."
    fi
    rm -rf "$OLD"
else
    mv "$NEW" "$APP"
fi

# ---- The rest, all under the user's folders ----
mkdir -p "$BIN" "$APPS" "$MIMEDIR/packages"
ln -sf "$APP/stagepack-studio" "$BIN/stagepack-studio"

: >"$APPS/stagepack-studio.desktop"
while IFS= read -r _line || [ -n "$_line" ]; do
    case $_line in
        *@EXEC@*) printf '%s%s%s\n' "${_line%%@EXEC@*}" "$EXEC_QUOTED" "${_line#*@EXEC@}" ;;
        *) printf '%s\n' "$_line" ;;
    esac
done <"$SRC/stagepack-studio.desktop.in" >"$APPS/stagepack-studio.desktop"

cp "$SRC/stagepack-studio-mime.xml" "$MIMEDIR/packages/stagepack-studio.xml"
for _n in $ICONS; do
    mkdir -p "$DATA/icons/hicolor/${_n}x${_n}/apps" "$DATA/icons/hicolor/${_n}x${_n}/mimetypes"
    cp "$SRC/icons/icon-$_n.png" "$DATA/icons/hicolor/${_n}x${_n}/apps/stagepack-studio.png"
    cp "$SRC/icons/icon-$_n.png" "$DATA/icons/hicolor/${_n}x${_n}/mimetypes/application-vnd.stagepack.talk.png"
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
