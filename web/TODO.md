# Review loop
- "Request review" button in UI (signals Claude to check)
- Track unresolved thread count per change
- Live updates
- MCP server doesn't reload when binary is rebuilt (need to restart Claude Code)
- Store commit_id in Review struct (so we know which revision feedback applies to)

# Small changes workflow
- Guide Claude to work in small chunks, wait for review
- Guide Claude to use jj and to move changes
- Change status: draft vs ready for review
- Revision concept (v1 → v2 → v3 responding to feedback)
- See comments move across changes/revisions
- Approve and bump main

## Ideas for review guidelines
- Keeping documentation up to date
- Explaining changes; no surprises

# Known limitations / tech debt
- Support commenting on deleted lines (old file)
- Handle race conditions in async store actions
- Duplicate components for threads

# Friction
- Click should move focus (like keyboard focus)
- Put location in URL (for refreshing)
- See if zustand can survive hot reloads better?
- Wrap long lines
- Navigating between comments in threads and code isn't perfect
- (Maybe) hide resolved changes in diffs?
- Resolving thread moves it, messing with navigation
- "?" to see hotkeys (maybe something cute to always show hints?)
- Change list is quite space-consuming

# Features
- Show file list in diff
- Expand diff blocks
- Search in viewers
- Hide changes to eg. lock files
- Let Claude explain tricky things / comment on changes as well?

# Bigger features
- Track task/context/issue for stack of changes
- Manage different changes / "branches"?
- Code viewer for current code (not just diffs)
- Show plan/track TODOs in UI
- Link back to conversations?
- Categorize / learn from similar comments over time

# Cute
- Dark mode

# Infrastructure
- Tests? End-to-end or UI?
