if [ -f "$HOME/.zshrc" ]; then
    source "$HOME/.zshrc"
fi

if typeset -f precmd > /dev/null; then
    functions[__nist_user_precmd]="${functions[precmd]}"
fi

precmd() {
    local exit_code=$?
    if [ $exit_code -ne 0 ]; then
        printf '\e[31m❌ Error code: %s\e[0m\n' "$exit_code"
    fi
    printf '\e]1337;command-exit=%s\a' "$exit_code"
    if typeset -f __nist_user_precmd > /dev/null; then
        __nist_user_precmd
    fi
}
