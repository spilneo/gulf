#!/bin/sh

set -eu

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null) || {
    printf '%s\n' 'error: install-git-hooks.sh must be run from within a Git repository' >&2
    exit 1
}

if [ "$script_dir" != "$repo_root/scripts" ]; then
    printf '%s\n' 'error: install-git-hooks.sh must be located in the repository scripts directory' >&2
    exit 1
fi

git -C "$repo_root" config --local core.hooksPath .githooks
printf '%s\n' 'Git hooks installed: core.hooksPath=.githooks'
