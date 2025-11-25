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

### Splitting changes after accidentally editing an existing commit
If you edited files without first running `jj new`, the changes went into the existing commit. To split them out:

1. `jj new -m "description of new changes"` - creates empty child commit
2. `jj evolog -r @-` - shows how the parent change evolved; find the commit ID before your edits
3. `jj restore --from <old-commit-id> --into @- --restore-descendants`

The `--restore-descendants` flag preserves the *content* of descendant commits (rather than their *diff*), so your new changes end up in the child commit.
