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
    set -l cd_file (mktemp)
    if test -z "$cd_file"
        command git $argv
        return $status
    end

    set -lx GIT_WS_CD_FILE "$cd_file"
    command git $argv
    set -l exit_code $status

    set -l target
    if test -s "$cd_file"
        read target < "$cd_file"
    end
    rm -f "$cd_file"

    if test $exit_code -eq 0 -a -n "$target" -a -d "$target"
        cd "$target"
        return 0
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
      local cd_file exit_code target
      cd_file="$(mktemp "${TMPDIR:-/tmp}/git-ws.XXXXXX")" || {
        command git "$@"
        return $?
      }
      GIT_WS_CD_FILE="$cd_file" command git "$@"
      exit_code=$?
      if [ -s "$cd_file" ]; then
        IFS= read -r target < "$cd_file"
      fi
      rm -f "$cd_file"
      if [ "$exit_code" -eq 0 ] && [ -n "${target:-}" ] && [ -d "$target" ]; then
        cd "$target" || return
        return 0
      fi
      return "$exit_code"
      ;;
  esac
  command git "$@"
}
"#;
