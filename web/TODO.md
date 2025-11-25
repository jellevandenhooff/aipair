# Review loop

# Revision tracking enhancements
- Thread position tracking/relocation based on diffs (threads "float" to new locations)
- "What's new since last review" banner
- Preserve jj revision history (prevent GC of old commits)

# Small changes workflow
- Guide Claude to work in small chunks, wait for review
- Guide Claude to use jj and to move changes
- Change status: draft vs ready for review
- Approve and bump main

## Ideas for review guidelines
- Keeping documentation up to date
- Explaining changes; no surprises

# Known limitations / tech debt
- Figure out what changes to show when using `jj edit` on an earlier change in the stack
- Support commenting on deleted lines (old file)
- Handle race conditions in async store actions (e.g., revision list shows stale data briefly when switching changes - fetch data first, then compute/display)
- Duplicate components for threads
- Decentralize state: store.ts is accumulating too much (e.g., newCommentText, confirm logic). Consider isDirty ref pattern or component-level state with callbacks.
- Consider removing line_end from Thread data model (we only use line_start now)

# Friction
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
