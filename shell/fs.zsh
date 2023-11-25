
_completes() {
    local code=$(fs --_shell zsh --_completes "${words}")
    eval $code
    return 0
}

compdef _completes fs