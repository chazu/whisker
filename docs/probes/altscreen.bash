# Probe design C: alternate screen overlay from bind -x.
[[ $- == *i* ]] || return
shopt -u promptvars
PS1='`-=> '
_prompt() { local s=$?; printf 'INFO ROW\n'; return $s; }
PROMPT_COMMAND=_prompt
_overlay() {
    local s=$?
    # Switch to the alternate screen, draw a full map, then restore.
    printf '\033[?1049h\033[H'
    printf 'FULL GRID MAP\n  ops  staging  prod\n  code  o  o  o\n  infra o  X  o\n'
    sleep 0.4
    printf '\033[?1049l'
    return $s
}
bind -x '"\e[99~":_overlay' 2>/dev/null
bind '"\eo":"\e[99~"' 2>/dev/null
