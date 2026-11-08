#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'EOF'
Usage:
  make contract-worktree-create CONTRACT_REPO=<repo> CONTRACT_BRANCH=<branch> WORKTREE_PATH=<path>
  make contract-worktree-remove WORKTREE_PATH=<path>
EOF
}

fail() {
    printf 'contract-worktree: %s\n' "$1" >&2
    exit 1
}

require_value() {
    local name="$1"
    local value="$2"
    [[ -n "$value" ]] || fail "$name is required"
}

create_worktree() {
    require_value CONTRACT_REPO "${CONTRACT_REPO:-}"
    require_value CONTRACT_BRANCH "${CONTRACT_BRANCH:-}"
    require_value WORKTREE_PATH "${WORKTREE_PATH:-}"

    local repo_root worktree_path
    repo_root="$(git -C "$CONTRACT_REPO" rev-parse --show-toplevel 2>/dev/null)" \
        || fail "CONTRACT_REPO must be an existing non-bare Git checkout"
    repo_root="$(realpath -e "$repo_root")"
    git -C "$repo_root" check-ref-format --branch "$CONTRACT_BRANCH" >/dev/null \
        || fail "CONTRACT_BRANCH is not a valid branch name"

    worktree_path="$(realpath -m "$WORKTREE_PATH")"
    [[ "$worktree_path" != "$repo_root" ]] || fail "WORKTREE_PATH cannot be the stable checkout"
    case "$worktree_path/" in
        "$repo_root/"*) fail "WORKTREE_PATH must be outside the stable checkout" ;;
    esac
    [[ ! -e "$worktree_path" ]] || fail "WORKTREE_PATH already exists; choose a new path"

    mkdir -p "$(dirname "$worktree_path")"
    if git -C "$repo_root" show-ref --verify --quiet "refs/heads/$CONTRACT_BRANCH"; then
        git -C "$repo_root" worktree add "$worktree_path" "$CONTRACT_BRANCH"
    else
        git -C "$repo_root" worktree add -b "$CONTRACT_BRANCH" "$worktree_path" HEAD
    fi
    printf 'Created worktree %s on branch %s\n' "$worktree_path" "$CONTRACT_BRANCH"
}

remove_worktree() {
    require_value WORKTREE_PATH "${WORKTREE_PATH:-}"
    local worktree_path git_dir common_dir dirty
    [[ -d "$WORKTREE_PATH" ]] || fail "WORKTREE_PATH must name an existing worktree directory"
    worktree_path="$(realpath -e "$WORKTREE_PATH")"
    git -C "$worktree_path" rev-parse --show-toplevel >/dev/null 2>&1 \
        || fail "WORKTREE_PATH is not a Git checkout"

    git_dir="$(git -C "$worktree_path" rev-parse --path-format=absolute --git-dir)"
    common_dir="$(git -C "$worktree_path" rev-parse --path-format=absolute --git-common-dir)"
    [[ "$git_dir" != "$common_dir" ]] || fail "refusing to remove a primary checkout"

    dirty="$(git -C "$worktree_path" status --porcelain --untracked-files=all --ignored)"
    [[ -z "$dirty" ]] || fail "worktree has modified, untracked, or ignored files; preserve it and commit or move those files first"

    git -C "$worktree_path" worktree remove "$worktree_path"
    printf 'Removed clean worktree %s; its branch was retained\n' "$worktree_path"
}

[[ $# -eq 1 ]] || { usage; exit 2; }
case "$1" in
    create) create_worktree ;;
    remove) remove_worktree ;;
    *) usage; exit 2 ;;
esac
