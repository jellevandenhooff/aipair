- feedback thoughts: need to pin the "head" revision so when we edit it's ok?
- need to add some agent instructions (when we install? run aipair?)
    - teach it jj absorb
    - teach it to make edits commits at a time
    - teach it to fix rebase errors
    - ???
    - teach it to not do too many things at once?
    - teach it to write feedback incrementally?
    - maybe let it paginate?
    - encourage honesty and precision; "you are a professional engineer; we want small and correct chnages"
    - encourage addressing all comments
    - encourage either fixing it, or leaving TODO in the code
    - to the mcp tool, add info timestamps on comments?
    - to the mcp tool, add commit information with lines (so that claude also doesn't get confused)
    - to the mcp tool, add diff context (that i see but claude doesn't!!)
    - to the mcp tool, add support for tracked/not tracked changes? so we know what we are working on
    - when addressing feedback, tell it to consider either making the changes in commits, or on latest and then squashing (as appropriate)
    - tell it to be careful with restore and undo
    - STROSNGLY encourage working on feedback one-by-one (if possible), and addressing and making revisions (maybe add functionality for sub-revisions????), so that code "works at all times". 
        - add mcp tool for letting it mark things as active??? relating edits to specific feedback items???
    - tell not to respond to comments if it hasn't done it yet
- MAKE SURE main is immutable???
- use interdiff to check diffs between diffs
- move comments when diffs change?
- file headers in ui, navigate between files, make sure search works
- support markdown in comments
- in the MCP responses leave hints/reminders on what to do?


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
- Move "no commit message" check from frontend to backend (return merge eligibility info from API)

# Friction
- Put location in URL (for refreshing)
- See if zustand can survive hot reloads better?
- Wrap long lines
- Navigating between comments in threads and code isn't perfect
- (Maybe) hide resolved changes in diffs?
- Resolving thread moves it, messing with navigation
- "?" to see hotkeys (maybe something cute to always show hints?)
- Change list is quite space-consuming
- Add .aipair to .gitignore on install
- Add aipair slash command on install

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
