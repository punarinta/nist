__nist_report_exit() {
    local exit_code=$?
    local ignorable_codes=(130)  # 130 = SIGINT (Ctrl+C)

    local should_report=1
    for code in "${ignorable_codes[@]}"; do
        if [ $exit_code -eq $code ]; then
            should_report=0
            break
        fi
    done

    if [ $exit_code -ne 0 ] && [ $should_report -eq 1 ]; then
        printf '\e[31m❌ Error code: %s\e[0m\n' "$exit_code"
    fi
    printf '\e]1337;command-exit=%s\a' "$exit_code"
    return $exit_code
}

if [ -f "$HOME/.bashrc" ]; then
    source "$HOME/.bashrc"
fi

if [ -z "$PROMPT_COMMAND" ]; then
    PROMPT_COMMAND="__nist_report_exit"
else
    PROMPT_COMMAND="__nist_report_exit; $PROMPT_COMMAND"
fi
