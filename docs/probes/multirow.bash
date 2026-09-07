# Probe: can a multi-row block above the input be repainted from bind -x,
# preserving partly typed input and the cursor?
[[ $- == *i* ]] || return
shopt -u promptvars
PS1='`-=> '
_rows=3
_grid=("row A .....") 
_state=0
_render() {
    _grid=()
    for r in 0 1 2; do
        line=""
        for c in 0 1 2 3; do
            n=$((r*4+c))
            if [[ $n == $_state ]]; then line+="[#]"; else line+=" o "; fi
        done
        _grid+=("$line")
    done
}
_prompt() { local s=$?; _render; printf '%s\n' "${_grid[@]}"; return $s; }
_next() {
    local s=$?
    _state=$(( (_state+1) % 12 ))
    _render
    # Move up N rows, repaint each, come back down.
    printf '\033[%dA\r' "$_rows"
    for line in "${_grid[@]}"; do printf '\033[2K%s\n' "$line"; done
    printf '\r'
    return $s
}
PROMPT_COMMAND=_prompt
bind -x '"\e[99~":_next' 2>/dev/null
bind '"\eo":"\e[99~"' 2>/dev/null
