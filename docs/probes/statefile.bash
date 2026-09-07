# The reliable design: state lives in a file written by background workers;
# the prompt hook and any keybind read it. No async repaint needed.
[[ $- == *i* ]] || return
shopt -u promptvars
PS1='`-=> '
STATE=/tmp/whisker_grid_state
_read() { _grid_line=$(cat "$STATE" 2>/dev/null || echo "no state"); }
_prompt() { local s=$?; _read; printf '%s\n' "$_grid_line"; return $s; }
PROMPT_COMMAND=_prompt
_refresh() { local s=$?; _read; printf '\033[1A\r\033[2K%s\033[1B\r' "$_grid_line"; return $s; }
bind -x '"\e[99~":_refresh' 2>/dev/null
bind '"\eo":"\e[99~"' 2>/dev/null
