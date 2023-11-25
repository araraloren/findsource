
function _completes
    set COMP_LINE (commandline -cp)

    fs --_completes "$COMP_LINE" --_shell fish
end

complete --command fs --no-files --arguments "(_completes)"