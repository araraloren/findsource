
_completes() {
    COMPREPLY=($(fs --_completes "$COMP_LINE" --_shell bash))
}

complete -F _completes fs