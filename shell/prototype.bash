# PROTOTYPE: loaded by ./try-it in a disposable Bash 5+ session.
[[ $- == *i* ]] || return
if (( BASH_VERSINFO[0] < 5 )); then
    printf 'Whisker prototype requires Bash 5+.\n' >&2
    return 1
fi

_whisker_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
_whisker_bin=${WHISKER_BIN:-$_whisker_root/target/debug/whisker}
if [[ ! -x $_whisker_bin ]]; then
    printf 'Build Whisker first with ./try-it.\n' >&2
    return 1
fi

# These variables belong to this shell, not a shared state file.
# The configuration decides which view a new shell starts in. Report a broken
# configuration once, here, rather than on every prompt.
if ! _whisker_view=$("$_whisker_bin" view start 2>&1); then
    printf 'Whisker configuration error: %s\n' "$_whisker_view" >&2
    printf 'Check it with: %s config check\n' "$_whisker_bin" >&2
    return 1
fi
_whisker_header=
_whisker_columns=${COLUMNS:-80}
# The visible prompt has two rows. Emit the info row in the prompt hook, and
# give Readline a stable input-row PS1 so it cannot cache an obsolete info row.
shopt -u promptvars
PS1='`-=> '

_whisker_render() {
    local next
    if next=$("$_whisker_bin" render --view "$_whisker_view" --columns "${COLUMNS:-80}"); then
        _whisker_header=$next
    else
        _whisker_header='[whisker unavailable]'
    fi
    _whisker_columns=${COLUMNS:-80}
}

_whisker_prompt() {
    local previous_status=$?
    _whisker_render
    printf '%s\n' "$_whisker_header"
    return "$previous_status"
}

_whisker_clear() {
    local previous_status=$?
    _whisker_render
    # Readline redraws the input after this callback; reserve the info row.
    printf '\033[H\033[2J%s\n' "$_whisker_header"
    return "$previous_status"
}

_whisker_next() {
    local previous_status=$?
    _whisker_view=$("$_whisker_bin" view next --current "$_whisker_view") || return
    if [[ $_whisker_columns != "${COLUMNS:-80}" ]]; then
        # A resize can reflow the previous info row. Reestablish its position
        # with a full repaint rather than guessing how many rows it now spans.
        _whisker_clear
        return "$previous_status"
    fi
    _whisker_render
    # bind -x clears the visible input and moves to the first input row before
    # invoking us. Repaint the single row above it; Readline restores the input,
    # including wrapped text and cursor position. Do not touch READLINE_LINE.
    printf '\033[1A\r\033[2K%s\033[1B\r' "$_whisker_header"
    return "$previous_status"
}

# This rcfile owns a fresh shell's prompt. Full dotfile-hook composition is
# deliberately deferred until the interaction has been tried in a terminal.
PROMPT_COMMAND=_whisker_prompt
bind -x '"\e[99~":_whisker_next'
bind -x '"\e[98~":_whisker_clear'
bind -f "$_whisker_root/shell/prototype.inputrc"
# Announce the configured cycle rather than a fixed list of view names.
_whisker_cycle=$("$_whisker_bin" view list | paste -sd' ' - | sed 's/ / → /g')
printf 'Whisker: Alt-O cycles %s. Type exit to leave.\n' "$_whisker_cycle"
unset _whisker_cycle
