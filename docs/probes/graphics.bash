# Graphics in the prompt, in a real terminal.
#
# graphics.py answers what an emulator can. This answers what it cannot: does
# your terminal draw a sixel inside PS1, and what happens to the line you type?
#
#   /opt/homebrew/bin/bash --norc
#   source docs/probes/graphics.bash
#
# Then type a long command and press Ctrl-A, Ctrl-E, and Up. Watch the cursor.

# A small red-ish sixel block. Terminals without sixel print the payload as
# text, which is itself the result: the feature is not portable.
sixel=$'\ePq#0;2;100;0;0#0~~@@vv@@~~$-#0??}}GG}}??-\e\\'

# Ask the terminal what it is. A sixel-capable terminal includes ";4;" in the
# primary device attributes reply.
probe_da() {
  local reply
  printf '\e[c'
  read -rsd c -t 1 reply
  printf 'device attributes: %q\n' "$reply"
  case $reply in
    *';4;'*|*';4c'*) printf 'sixel: advertised\n' ;;
    *) printf 'sixel: NOT advertised\n' ;;
  esac
}

# The kitty protocol answers a different query; absence of a reply is a no.
probe_kitty() {
  local reply
  printf '\e_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\e\\\e[c'
  read -rsd c -t 1 reply
  case $reply in
    *_Gi=31*OK*) printf 'kitty graphics: supported\n' ;;
    *) printf 'kitty graphics: no OK response\n' ;;
  esac
}

# Case 1: the honest prompt. The escape is NOT wrapped in \[ \], so Readline
# counts every byte of the payload as a printable column. Expect the cursor to
# be wildly wrong.
honest() { PS1="${sixel}\$ "; }

# Case 2: the usual fix. Wrapped in \[ \], so Readline counts it as zero
# columns. But the image occupies real cells, so the count is still wrong,
# just in the other direction.
hidden() { PS1="\[${sixel}\]\$ "; }

# Case 3: pad the lie. Claim the image is zero-width, then spend real spaces to
# cover the cells it actually occupies. Only works if you know the cell size,
# which depends on the font.
padded() { PS1="\[${sixel}\]    \$ "; }

printf 'TERM=%s TERM_PROGRAM=%s\n' "$TERM" "$TERM_PROGRAM"
probe_da
probe_kitty
cat <<'EOF'

Try each, then type a long command and press Ctrl-A / Ctrl-E / Up:

  honest   escape unwrapped; Readline counts the payload bytes as columns
  hidden   escape in \[ \]; Readline counts it as zero
  padded   zero-width plus hand-counted spaces

What to record: whether an image appears at all, whether it survives Ctrl-L and
a resize, and whether the cursor tracks the text after Ctrl-A.
EOF
