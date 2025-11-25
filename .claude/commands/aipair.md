Check for review feedback and respond to comments.

Use `mcp__aipair__get_pending_feedback` to see open threads with code context.

For each thread:
1. First, do the work to address the feedback (if applicable)
2. IMPORTANT: Double-check that ALL work is actually complete before responding
3. Then use `mcp__aipair__respond_to_thread` to reply

After addressing feedback:
- If you made code changes and responded to multiple threads, use `mcp__aipair__record_revision` to create a snapshot
- This helps track what changed between review rounds

Guidelines:
- VERIFY your work is done before responding - re-read the feedback and confirm each point was addressed
- Only set resolve=true if you are CERTAIN the feedback was addressed as the user intended
- If you disagree or need clarification, reply explaining your reasoning (don't resolve)
- If feedback was surprising and might reoccur, document it in project guidelines (CLAUDE.md)
- If something should be remembered for later, put it in TODO.md or a code comment - review conversations are ephemeral
- Be concise - don't say "will think about" or "adding to considerations", either do it now or add to TODO.md
