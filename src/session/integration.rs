//! Shell integration scripts for session mode.
//!
//! These scripts are minimal: they only emit OSC markers carrying the
//! current command line buffer and cursor position. All keystroke handling,
//! popup rendering, and navigation is done by the PTY wrapper.

/// Generate the bash integration script for session mode.
///
/// This script:
/// 1. Binds printable chars via `bind -x` to emit OSC markers after each edit
/// 2. Emits PromptStart/PromptEnd markers via PROMPT_COMMAND
/// 3. Does NOT handle popup rendering or navigation (PTY wrapper does that)
pub fn bash_integration() -> String {
    r##"
# Tabra PTY Session Integration for Bash
# Emits OSC markers; does NOT bind keys for popup (PTY wrapper handles that)

__tabra_report_cmdline() {
    local b64
    b64=$(printf '%s' "$READLINE_LINE" | base64 -w0 2>/dev/null || printf '%s' "$READLINE_LINE" | base64 2>/dev/null)
    printf '\033]6973;CL;%s;%d\007' "$b64" "$READLINE_POINT"
}

__tabra_prompt_start() {
    printf '\033]6973;PS\007'
}

__tabra_prompt_end() {
    printf '\033]6973;PE\007'
}

# Self-insert wrapper: insert char into READLINE_LINE then report
__tabra_si() {
    local c="$1"
    local before="${READLINE_LINE:0:$READLINE_POINT}"
    local after="${READLINE_LINE:$READLINE_POINT}"
    READLINE_LINE="${before}${c}${after}"
    (( READLINE_POINT += ${#c} ))
    __tabra_report_cmdline
}

# Backward delete wrapper: delete char then report
__tabra_bd() {
    if (( READLINE_POINT > 0 )); then
        READLINE_LINE="${READLINE_LINE:0:$((READLINE_POINT-1))}${READLINE_LINE:$READLINE_POINT}"
        (( READLINE_POINT-- ))
    fi
    __tabra_report_cmdline
}

# Bind printable characters
__tabra_bind() {
    local chars="abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    chars+="-./_~:=+@!#%"
    chars+=" "
    local i
    for (( i = 0; i < ${#chars}; i++ )); do
        local c="${chars:$i:1}"
        bind -x "\"$c\": __tabra_si '$c'"
    done
}
__tabra_bind

# Backspace
bind -x '"\C-?": __tabra_bd'

# Prompt markers
if [[ -z "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__tabra_prompt_start"
else
    PROMPT_COMMAND="__tabra_prompt_start;${PROMPT_COMMAND}"
fi
PS1="${PS1}\$(__tabra_prompt_end)"
"##
    .to_string()
}

/// Generate the zsh integration script for session mode.
pub fn zsh_integration() -> String {
    r##"
# Tabra PTY Session Integration for Zsh
# Emits OSC markers via ZLE widgets

__tabra_report() {
    local b64
    b64=$(printf '%s' "$BUFFER" | base64 2>/dev/null)
    printf '\033]6973;CL;%s;%d\007' "$b64" "$CURSOR"
}

__tabra_si() {
    zle .self-insert
    __tabra_report
}

__tabra_bd() {
    zle .backward-delete-char
    __tabra_report
}

zle -N __tabra_si
zle -N __tabra_bd

() {
    local -a keys
    keys=({a..z} {A..Z} {0..9} '-' '_' '.' '/' '~' ':' '=' '+' '@' '!' '#' '%' ' ')
    for key in "${keys[@]}"; do
        bindkey -M main "$key" __tabra_si
    done
}
bindkey -M main '^?' __tabra_bd

precmd() { printf '\033]6973;PS\007' }
preexec() { printf '\033]6973;PE\007' }
"##
    .to_string()
}

/// Generate the fish integration script for session mode.
pub fn fish_integration() -> String {
    r##"
# Tabra PTY Session Integration for Fish
# Emits OSC markers via key bindings

function __tabra_report
    set -l b64 (printf '%s' (commandline --current-buffer) | base64 2>/dev/null)
    set -l cursor (commandline --cursor)
    printf '\033]6973;CL;%s;%d\007' "$b64" "$cursor"
end

function __tabra_si
    commandline -i -- $argv[1]
    __tabra_report
end

function __tabra_bd
    commandline -f backward-delete-char
    __tabra_report
end

for c in a b c d e f g h i j k l m n o p q r s t u v w x y z \
         A B C D E F G H I J K L M N O P Q R S T U V W X Y Z \
         0 1 2 3 4 5 6 7 8 9 \
         - . / _ '~' : = + @ '!' '#' '%' ' '
    bind $c "__tabra_si $c"
end
bind \x7f __tabra_bd

function __tabra_prompt_start --on-event fish_prompt
    printf '\033]6973;PS\007'
end
function __tabra_prompt_end --on-event fish_postexec
    printf '\033]6973;PE\007'
end
"##
    .to_string()
}
