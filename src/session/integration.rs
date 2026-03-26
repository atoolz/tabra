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
# Source user's bashrc first so we inherit their prompt, aliases, etc.
if [[ -f ~/.bashrc ]]; then
    source ~/.bashrc
fi

# Prompt markers (OSC sequences stripped by the PTY wrapper)
__tabra_prompt_start() { printf '\033]6973;PS\007'; }
__tabra_prompt_end() { printf '\033]6973;PE\007'; }

if [[ -z "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__tabra_prompt_start"
elif [[ "$PROMPT_COMMAND" != *"__tabra_prompt_start"* ]]; then
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
    set -l buf (commandline --current-buffer)
    set -l b64 (printf '%s' "$buf" | base64 2>/dev/null)
    # NOTE: commandline --cursor returns codepoint offset, not byte offset.
    # For ASCII-only input (99% of CLI commands) these are identical.
    # Multi-byte characters before the cursor will cause a mismatch.
    # TODO: convert codepoint offset to byte offset for full UTF-8 support.
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
function __tabra_prompt_end --on-event fish_preexec
    printf '\033]6973;PE\007'
end
"##
    .to_string()
}
