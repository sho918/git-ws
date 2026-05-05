use anyhow::{Result, anyhow};

pub fn init_script(shell: &str) -> Result<&'static str> {
    match shell {
        "fish" => Ok(FISH),
        "zsh" | "bash" => Ok(POSIX),
        other => Err(anyhow!("unsupported shell: {other}")),
    }
}

const FISH: &str = r#"# git-ws shell integration for fish
function __git_ws_run_and_cd
    set -l result (command git $argv)
    set -l exit_code $status
    if test $exit_code -eq 0 -a (count $result) -gt 0
        set -l last_line $result[-1]
        if test -d "$last_line"
            for line in $result[1..-2]
                printf "%s\n" "$line"
            end
            cd "$last_line"
            return 0
        end
    end
    for line in $result
        printf "%s\n" "$line"
    end
    return $exit_code
end

function git --wraps git
    switch "$argv[1]"
        case ws co main master
            __git_ws_run_and_cd $argv
            return $status
    end
    command git $argv
end
"#;

const POSIX: &str = r#"# git-ws shell integration for sh-compatible shells
git() {
  case "$1" in
    ws|co|main|master)
      local result exit_code last_line
      result="$(command git "$@")"
      exit_code=$?
      last_line="$(printf '%s\n' "$result" | tail -n 1)"
      if [ "$exit_code" -eq 0 ] && [ -d "$last_line" ]; then
        printf '%s\n' "$result" | sed '$d'
        cd "$last_line" || return
        return 0
      fi
      printf '%s\n' "$result"
      return "$exit_code"
      ;;
  esac
  command git "$@"
}
"#;
