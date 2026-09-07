[[ $- == *i* ]] || return
shopt -u promptvars
PS1='`-=> '
_alert=none
_paint() { printf '\033[s\033[1A\r\033[2KGRID alert=%s\033[u' "$_alert"; }
_prompt() { local s=$?; printf 'GRID alert=%s\n' "$_alert"; return $s; }
PROMPT_COMMAND=_prompt
_on_usr1() { _alert=USR1; _paint; }
trap _on_usr1 SIGUSR1
