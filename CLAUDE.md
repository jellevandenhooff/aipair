# Project Guidelines

## TODO Lists
- When a TODO item is complete, remove it from the list entirely
- Do not mark items as done with strikethrough or checkmarks
- If removing feels wrong (e.g., might want to track what was done), consider if it belongs in commit messages or documentation instead

## Version Control
This project uses jj (Jujutsu) for version control.

- Before making changes: run `jj new` to create a fresh change (otherwise edits go into the existing working copy)
- To "commit": run `jj describe -m "message"` then `jj new` to finalize and start fresh
- Set a meaningful description early and update it as you work - the commit message is part of the review in aipair
- ALWAYS pass `-m "message"` to jj commands that accept it (describe, new, squash) to avoid opening an editor

### Splitting changes after accidentally editing an existing commit
If you edited files without first running `jj new`, the changes went into the existing commit. To split them out:

1. `jj new -m "description of new changes"` - creates empty child commit
2. `jj evolog -r @-` - shows how the parent change evolved; find the commit ID before your edits
3. `jj restore --from <old-commit-id> --into @- --restore-descendants`

The `--restore-descendants` flag preserves the *content* of descendant commits (rather than their *diff*), so your new changes end up in the child commit.

### Addressing review feedback
When fixing review feedback, use `jj edit` to directly edit the change being reviewed:

1. `jj edit <change-id>` - check out the change to edit
2. Make the changes (they go directly into that change)
3. `jj edit <original-change>` - return to where you were working

This keeps the fix in the original change rather than creating a separate "fix feedback" commit.

## Development

### Dev workflow

`just dev /path/to/test-repo` starts everything needed for development:
- `cargo watch` auto-rebuilds the binary on source changes
- `aipair serve` in the test repo with auto-port (auto-restarts when binary is rebuilt)
- Vite dev server for frontend hot-reload (proxies API to the auto-assigned port)
- Adds `target/debug` to the test repo's PATH via `.envrc`

Multiple dev servers can run simultaneously — each gets its own port.

`just dev-teardown /path/to/test-repo` removes the `.envrc` changes.

### Server port conventions

- **Auto-port** (no `--port` flag): tries `.aipair/port`, then OS-assigned. Writes port to `.aipair/port` for reuse.
- **Explicit `--port N`**: uses that port exactly. Does not write port file.
