# Stagepack Studio: where everything goes. Sourced by install.sh and uninstall.sh (never run alone),
# so both work out the same places by the same rules. The caller defines `refuse` first.
#
# Reads HOME and the XDG_* variables; sets HOME (trimmed), DATA, CONFIG, BIN, APP, APPS, MIMEDIR.

NL='
'
TYPE=application/vnd.stagepack.talk
ENTRY=stagepack-studio.desktop
MARK=X-Stagepack-Studio=1

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

# A full path with no line break and no "." or ".." part.
plain_path() {
    case $1 in
        /*) ;;
        *) return 1 ;;
    esac
    case $1 in
        *"$NL"* | */./* | */. | */../* | */..) return 1 ;;
    esac
    return 0
}

[ -n "${HOME:-}" ] || refuse "HOME isn't set, so there is no home folder to work in."
HOME=$(trim "$HOME")
[ -n "$HOME" ] || refuse "HOME is the top of the disk, which is not a home folder."
plain_path "$HOME" || refuse "HOME isn't a plain full path, so there is no clear home folder."
[ "$(id -u)" != 0 ] || refuse "This is for one user and must not be run as root; run it as the user who will use the program."

# Each XDG_* folder: a plain full path, inside the home folder.
for _var in XDG_DATA_HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_BIN_HOME; do
    eval "_val=\${$_var:-}"
    [ -n "$_val" ] || continue
    _val=$(trim "$_val")
    plain_path "$_val" || refuse "$_var has to be a plain full path, with no relative parts like . or .."
    case $_val in "$HOME"/*) ;; *) refuse "$_var has to be a folder inside your home folder." ;; esac
done

if [ -n "${XDG_DATA_HOME:-}" ]; then
    DATA=$(trim "$XDG_DATA_HOME")
    APP="$(dirname "$DATA")/lib/stagepack-studio"
else
    DATA="$HOME/.local/share"
    APP="$HOME/.local/lib/stagepack-studio"
fi
CONFIG=$(trim "${XDG_CONFIG_HOME:-$HOME/.config}")
BIN=$(trim "${XDG_BIN_HOME:-$HOME/.local/bin}")
APPS="$DATA/applications"
MIMEDIR="$DATA/mime"

# Nothing is ever written in the app's own data folder (the usual place, or beside the chosen data folder).
for _p in "$APP" "$DATA" "$CONFIG" "$BIN" "$APPS" "$MIMEDIR" "$DATA/icons"; do
    plain_path "$_p" || refuse "A folder to be used isn't a plain full path."
    case "$_p/" in "$HOME/"*) ;; *) refuse "$_p is outside your home folder." ;; esac
    case "$_p/" in
        "$HOME/.local/share/stagepack-studio/"* | "$DATA/stagepack-studio/"*) refuse "$_p is the app's data folder or inside it, and that is never written to." ;;
    esac
done
case "$DATA/" in "$APP/"*) refuse "The data folder is inside where the program would go." ;; esac
