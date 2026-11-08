#!/usr/bin/env python3
"""Exercise the contract worktree targets against an isolated temporary repo."""

from __future__ import annotations

from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def run(command: list[str], *, cwd: Path | None = None,
        check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=check)


def make_target(target: str, **variables: str) -> subprocess.CompletedProcess[str]:
    command = ["make", "-C", str(ROOT), target]
    command.extend(f"{name}={value}" for name, value in variables.items())
    return run(command)


def git(repo: Path, *arguments: str) -> str:
    return run(["git", "-C", str(repo), *arguments]).stdout.strip()


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="open-contract-worktree-test-") as temporary:
        temp_root = Path(temporary)
        repo = temp_root / "stable-contract"
        worktree = temp_root / "feature-worktree"
        nested = repo / "must-not-be-created"
        repo.mkdir()
        run(["git", "init", "--initial-branch=main", str(repo)])
        git(repo, "config", "user.name", "Contract Worktree Test")
        git(repo, "config", "user.email", "contract-worktree-test@example.invalid")
        (repo / ".gitignore").write_text("ignored.tmp\n", encoding="utf-8")
        (repo / "baseline.txt").write_text("stable\n", encoding="utf-8")
        run(["git", "-C", str(repo), "add", ".gitignore", "baseline.txt"])
        run(["git", "-C", str(repo), "commit", "-m", "initial contract"])
        stable_head = git(repo, "rev-parse", "HEAD")

        occupied = temp_root / "occupied-path"
        occupied.mkdir()
        marker = occupied / "keep.txt"
        marker.write_text("preserve existing destination\n", encoding="utf-8")
        rejected_existing = run(
            ["make", "-C", str(ROOT), "contract-worktree-create",
             f"CONTRACT_REPO={repo}", "CONTRACT_BRANCH=feature/occupied",
             f"WORKTREE_PATH={occupied}"],
            check=False,
        )
        assert rejected_existing.returncode != 0, "creation into an existing path was allowed"
        assert marker.read_text(encoding="utf-8") == "preserve existing destination\n"

        rejected_nested = run(
            ["make", "-C", str(ROOT), "contract-worktree-create",
             f"CONTRACT_REPO={repo}", "CONTRACT_BRANCH=feature/nested",
             f"WORKTREE_PATH={nested}"],
            check=False,
        )
        assert rejected_nested.returncode != 0, "creation inside the stable checkout was allowed"
        assert not nested.exists(), "rejected creation changed the stable checkout"

        make_target(
            "contract-worktree-create",
            CONTRACT_REPO=str(repo),
            CONTRACT_BRANCH="feature/worktree-test",
            WORKTREE_PATH=str(worktree),
        )
        assert (worktree / ".git").is_file(), "created path is not a linked worktree"
        assert git(worktree, "branch", "--show-current") == "feature/worktree-test"
        assert git(repo, "rev-parse", "HEAD") == stable_head
        assert git(repo, "status", "--porcelain") == "", "stable checkout changed during creation"

        primary_removal = run(
            ["make", "-C", str(ROOT), "contract-worktree-remove", f"WORKTREE_PATH={repo}"],
            check=False,
        )
        assert primary_removal.returncode != 0, "primary checkout removal was allowed"
        assert repo.exists(), "primary checkout was removed"

        created_file = worktree / "work.txt"
        created_file.write_text("worktree content\n", encoding="utf-8")
        dirty_removal = run(
            ["make", "-C", str(ROOT), "contract-worktree-remove", f"WORKTREE_PATH={worktree}"],
            check=False,
        )
        assert dirty_removal.returncode != 0, "dirty worktree removal was allowed"
        assert created_file.exists(), "dirty worktree content was removed"

        run(["git", "-C", str(worktree), "add", "work.txt"])
        run(["git", "-C", str(worktree), "commit", "-m", "work in isolated checkout"])
        committed_head = git(worktree, "rev-parse", "HEAD")
        (worktree / "ignored.tmp").write_text("ignored content\n", encoding="utf-8")
        ignored_removal = run(
            ["make", "-C", str(ROOT), "contract-worktree-remove", f"WORKTREE_PATH={worktree}"],
            check=False,
        )
        assert ignored_removal.returncode != 0, "worktree removal discarded ignored content"
        assert (worktree / "ignored.tmp").exists(), "ignored worktree content was removed"

        (worktree / "ignored.tmp").unlink()
        make_target("contract-worktree-remove", WORKTREE_PATH=str(worktree))
        assert not worktree.exists(), "clean worktree removal did not remove the checkout"
        assert git(repo, "rev-parse", "refs/heads/feature/worktree-test") == committed_head
        run(["git", "-C", str(repo), "cat-file", "-e", f"{committed_head}^{{commit}}"])
        assert git(repo, "rev-parse", "HEAD") == stable_head
        assert git(repo, "status", "--porcelain") == "", "stable checkout changed during the lifecycle"

        existing_branch_path = temp_root / "existing-branch-worktree"
        make_target(
            "contract-worktree-create",
            CONTRACT_REPO=str(repo),
            CONTRACT_BRANCH="feature/worktree-test",
            WORKTREE_PATH=str(existing_branch_path),
        )
        assert git(existing_branch_path, "rev-parse", "HEAD") == committed_head
        make_target("contract-worktree-remove", WORKTREE_PATH=str(existing_branch_path))
        assert git(repo, "status", "--porcelain") == "", "stable checkout changed when reusing a branch"
        print("contract worktree lifecycle checks passed")


if __name__ == "__main__":
    main()
