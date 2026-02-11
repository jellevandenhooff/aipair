# Sessions: Isolated Clones for AI-Assisted Development

## Problem

Today in aipair, Claude and the user share a single working copy. This creates several friction points:

- **No isolation**: Claude's edits immediately affect the user's working tree. Accidental edits to the wrong jj change are easy and hard to undo.
- **Scope is implicit**: "Topics" group changes by metadata, but there's no physical boundary. Claude can touch anything.
- **Review is after-the-fact**: the user sees changes only after Claude has made them in the shared tree. There's no natural checkpoint for "here's what I did, please review."
- **Timeline is reconstructed**: we built a timeline by scraping Claude Code session files and hooking into review operations. But the actual development story (what changed when, and why) isn't a first-class concept.

## Model: Every unit of work is a session

A **session** is an isolated git clone where Claude (or the user) works. Changes flow between the clone and the main repo via explicit `push` and `pull` commands.

```
┌─────────────────────┐          push           ┌──────────────────────┐
│    Session Clone     │  ──────────────────────► │     Main Repo        │
│                      │                          │                      │
│  Claude works here   │  ◄────────────────────── │  User reviews here   │
│  jj repo (full)      │          pull            │  aipair server runs  │
│                      │                          │  .aipair/ metadata   │
└─────────────────────┘                          └──────────────────────┘
```

Key properties:
- **True isolation**: the clone is a separate git repo. Could be local, in a container, or on a remote machine.
- **Explicit checkpoints**: `push` is a deliberate act with a summary message. It creates a reviewable unit.
- **Natural scoping**: everything in the clone belongs to one session. No accidental cross-contamination.
- **Push history IS the timeline**: each push records what changed and why. No need to reconstruct the story.

## Session Lifecycle

```
create  →  work  →  push  →  review  →  pull  →  work  →  push  →  merge
```

### Create

From the main repo:
```
aipair session new fix-auth
```

This:
1. Creates a git clone of the main repo (e.g., `.aipair/sessions/fix-auth/repo/`)
2. Sets up a session bookmark (`session/fix-auth`)
3. Records session metadata in `.aipair/sessions/fix-auth.json`
4. Drops a marker file (`.aipair-session.json`) in the clone so the CLI knows where it is

### Work

Claude (or the user) works in the clone:
```
cd .aipair/sessions/fix-auth/repo/
# edit files, jj describe, jj new, etc.
```

The clone is a full jj repo. All normal jj operations work.

### Push

From the clone:
```
aipair push -m "Rewrote token validation to check expiry"
```

This:
1. Sets the session bookmark to the current change
2. Runs `jj git push` to send the bookmark to the main repo
3. Records a push event (summary, commit, timestamp) in the session metadata
4. The main repo now sees the session's changes in `jj log`

### Review

The user reviews in the web UI (unchanged from today):
- Pushed changes appear in the change list, tagged with the session name
- User adds review comments using the existing review UI
- Comments are stored in `.aipair/reviews/` in the main repo

### Pull

From the clone:
```
aipair pull
```

This:
1. Runs `jj git fetch` to get latest changes from main
2. Rebases session work onto the new main tip (if main advanced)
3. Fetches review comments from the aipair server (or reads from main repo)
4. Prints a summary: new commits from main, open review threads

### Merge

From the main repo:
```
aipair session merge fix-auth
```

This:
1. Moves the `main` bookmark to the session's tip
2. Deletes the session bookmark
3. Marks the session as merged
4. Optionally cleans up the clone directory

## CLI Design

The CLI has two contexts: **main repo** and **session clone**. It detects context via the `.aipair-session.json` marker file.

### From main repo
```
aipair session new <name>     Create a new session (clone + setup)
aipair session list           List sessions with status and push history
aipair session merge <name>   Land a session's changes onto main
```

### From session clone
```
aipair push -m "summary"     Push changes to main with a summary
aipair pull                   Fetch latest main + review comments
aipair status                 Show session info, pending changes, open threads
```

`push`, `pull`, and `status` are top-level commands (not under `session`) because you type them frequently from a clone.

## Data Model

### Session metadata (main repo)

``.aipair/sessions/<name>.json``:
```json
{
  "name": "fix-auth",
  "clone_path": ".aipair/sessions/fix-auth/repo",
  "bookmark": "session/fix-auth",
  "base_change_id": "abc123...",
  "status": "active",
  "created_at": "2025-01-15T10:00:00Z",
  "pushes": [
    {
      "summary": "Rewrote token validation",
      "change_id": "def456...",
      "commit_id": "789abc...",
      "timestamp": "2025-01-15T11:30:00Z"
    }
  ]
}
```

### Clone marker (clone root)

``.aipair-session.json``:
```json
{
  "session_name": "fix-auth",
  "main_repo": "/absolute/path/to/main/repo",
  "bookmark": "session/fix-auth"
}
```

## jj/git Mechanics

Sessions use jj's git interop for push/pull. The clone is created with `jj git clone` (or `git clone` + `jj git init --colocate`).

### Bookmarks

Each session has a bookmark named `session/<name>`. This is the branch that gets pushed/pulled.

- **Push**: `jj bookmark set session/<name> -r @` then `jj git push --bookmark session/<name>`
- **Pull**: `jj git fetch` (fetches all branches from origin, including `main`)
- **Merge**: `jj bookmark set main -r session/<name>` then `jj bookmark delete session/<name>`

### Rebase

After pulling, session changes may need rebasing onto the new main tip:
```
jj rebase -r <session-changes> -d main
```

jj handles this natively and will flag conflicts if they arise.

## Relationship to Existing Concepts

### Sessions replace topics

Topics are lightweight metadata grouping changes. Sessions are the same concept with real infrastructure (a clone, push/pull lifecycle, isolation). Topics can be deprecated once sessions are working.

### Reviews stay as-is

The existing review model (comments on changes, threads, resolve/reopen) works unchanged. When Claude pushes changes from a session, those changes appear in the main repo's `jj log`. The review UI shows them just like any other changes. The only addition: changes can be tagged with their session name.

### Timeline becomes push history

Instead of a separate append-only JSONL event log, the timeline is the push history. Each push has a summary, timestamp, and commit reference. Review comments and chat messages can be layered on top, scoped to a session.

## Future Directions

### Launching Claude in sessions

`aipair session new` could launch Claude Code inside the clone:
```
aipair session new fix-auth --prompt "Fix the auth token expiry bug"
```

This creates the clone, then runs `claude` in it with the prompt. The user watches progress in the web UI.

### Container isolation

Since the clone is a separate git repo, it can live anywhere:
- In a Docker container (mount the clone, expose git remote)
- On a remote machine (clone over SSH)
- In a sandboxed environment with restricted filesystem access

### Web UI integration

- Session list in the sidebar (instead of or alongside the change list)
- Push history as a timeline within each session
- "New session" button that creates a clone and optionally launches Claude
- Embedded terminal (Ghostty Web?) showing Claude's work in real-time

### Concurrent sessions

Multiple sessions can exist simultaneously. Each is an independent clone with its own bookmark. Merging is sequential (one at a time onto main) to avoid conflicts.

## Validated Plumbing (prototype results)

All core jj/git operations were tested in `session-proto/`:

| Operation | Result |
|-----------|--------|
| `jj git clone` of colocated repo | Works out of the box |
| Push bookmark to non-bare repo | Works (needs `--allow-new` for first push) |
| Main auto-imports pushed bookmarks | Yes, on next jj operation |
| `jj git fetch` picks up main's new commits | Yes |
| Rebase session onto fetched main | Clean rebase, no issues |
| Push again after rebase | jj handles "move sideways" automatically |
| Move main bookmark as merge | Works, clean result |

### Gotchas

- **Empty working copy change**: `jj git clone` creates an empty working copy change that blocks push (jj refuses to push undescribed commits). Session creation should squash or abandon it.
- **First push**: Needs `--allow-new` flag since the bookmark doesn't exist on the remote yet. Subsequent pushes don't need it.
- **Diverged bookmark**: After rebase, the bookmark shows `*` (diverged from remote) but push handles it without needing force flags.

## Open Questions

1. **Stacked changes**: A session may have multiple jj changes (a stack). The bookmark points to the tip. Does pushing the bookmark push the entire ancestry? (Likely yes — git push sends reachable commits.)

2. **Clone storage**: Clones inside `.aipair/sessions/` must be gitignored. Should we support configurable clone paths for container/remote scenarios?

3. **Conflict handling**: What happens when rebase produces conflicts? jj flags them — the `aipair pull` command should report conflicts clearly and not silently proceed.
