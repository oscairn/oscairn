---
title: "Agents in worktrees: no git stash, commit before resetting files, tests never write the user's real data folder"
author: Jacob
date: 2026-11-14
branch: main
cost: { minutes: 30, tokens: 53331 }
id: l-e49e37a2
scope: repo
status: active
src: agent
touches:
  - ui/PROGRESS.md
  - CMakeLists.txt
  - docs/rfcs/0030-system-shell-launcher-and-navigation.md
  - docs/rust-ui.md
  - ui/CMakeLists.txt
  - ui/src/tests.rs
  - devices/roland-fantom0/bundle/oscairn-fantom-start
  - devices/roland-fantom0/bundle/oscairn-fantom-stop
  - devices/roland-fantom0/tests/fantom_start.sh
  - docs/fantom-0.md
---

## Decided
- Never use `git stash` in this repo's worktrees. The stash stack is shared by the main checkout and every worktree under .claude/worktrees, and several agents work at once: a pop or drop can take another agent's entry. Set work aside with a work-in-progress commit on your own branch.
- Commit before any `git checkout -- <files>` or reset. On 2026-11-15 an agent reset 12 files to test one commit alone; the only copy of that work was loose files in /tmp until it was copied back.
- A test must never read or write the user's real Oscairn folders (on macOS ~/Library/Application Support/Oscairn). Point OSCAIRN_DATA_HOME and OSCAIRN_CONFIG_HOME at a temporary folder before anything reads them. On 2026-11-14 a new Rust UI test overwrote the Setlist app's state.json there and the old content was lost.

## Tried and ruled out
- Keeping "before" copies in /tmp and restoring them by hand: it works until an interruption, and then the work is in no repository.

## Watch out
- To watch a test fail without its fix, revert one file that has no other uncommitted work, or build the "before" state in a second worktree.
- Keep scratch files in an untracked folder inside your own worktree.

In the code: worktrees under .claude/worktrees; ui/ tests set their folders since commit 323fc564; the C++ desktop tests' temp-folder setup in hosts/standalone/tests.
