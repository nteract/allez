---
description: Create or update the feature specification from a natural language feature description.
handoffs:
  - label: Build Technical Plan
    agent: speckit.plan
    prompt: Create a plan for the spec. I am building with...
  - label: Clarify Spec Requirements
    agent: speckit.clarify
    prompt: Clarify specification requirements
    send: true
---

## User Input

```text
$ARGUMENTS
```

You **MUST** consider the user input before proceeding (if not empty).

## Pre-Execution Checks

**Check for extension hooks (before specification)**:
- Check if `.specify/extensions.yml` exists in the project root.
- If it exists, read it and look for entries under the `hooks.before_specify` key
- If the YAML cannot be parsed or is invalid, skip hook checking silently and continue normally
- Filter out hooks where `enabled` is explicitly `false`. Treat hooks without an `enabled` field as enabled by default.
- For each remaining hook, do **not** attempt to interpret or evaluate hook `condition` expressions:
  - If the hook has no `condition` field, or it is null/empty, treat the hook as executable
  - If the hook defines a non-empty `condition`, skip the hook and leave condition evaluation to the HookExecutor implementation
- For each executable hook, output the following based on its `optional` flag:
  - **Optional hook** (`optional: true`):
    ```
    ## Extension Hooks

    **Optional Pre-Hook**: {extension}
    Command: `/{command}`
    Description: {description}

    Prompt: {prompt}
    To execute: `/{command}`
    ```
  - **Mandatory hook** (`optional: false`):
    ```
    ## Extension Hooks

    **Automatic Pre-Hook**: {extension}
    Executing: `/{command}`
    EXECUTE_COMMAND: {command}

    Wait for the result of the hook command before proceeding to the Outline.
    ```
    After emitting the block above you MUST actually invoke the hook and wait for it to finish before continuing. Run it the same way you would run the command yourself in this agent/session (the invocation may differ from the literal `{command}` id shown above, e.g. a skills-mode agent runs it as `/skill:speckit-...` or `$speckit-...`). Emitting the block alone does not run the hook.
- If no hooks are registered or `.specify/extensions.yml` does not exist, skip silently

## Outline

The text the user typed after `/speckit.specify` in the triggering message **is** the feature description. Assume you always have it available in this conversation even if `$ARGUMENTS` appears literally below. Do not ask the user to repeat it unless they provided an empty command.

Given that feature description, do this:

0. **Check for a JIRA ticket in the description**:
   - Look for a ticket in the format `GEN-XXXX` (1-5 digits, case-insensitive) anywhere in the feature description.
   - If MULTIPLE tickets are found: ERROR "Please provide exactly one JIRA ticket per feature."
   - If exactly ONE ticket is found: note it (uppercased) as `JIRA_TICKET` and proceed to step 1.
   - If NO ticket is found, proceed to step 0b.

0b. **Ask the user for a JIRA ticket** (only if step 0 found none):
   - Present this prompt to the user:

     ```markdown
     I didn't find a JIRA ticket in your feature description.

     **Please provide one of the following:**
     1. A JIRA ticket number (e.g., `GEN-42`)
     2. Type "no ticket" if you don't have one

     **Note**: Never use placeholder ticket numbers. If you don't have a ticket yet, that's okay - just say "no ticket".
     ```

   - Wait for the user's response.
   - If the user provides a ticket (e.g., "GEN-42"): validate it matches `GEN-XXXX` (1-5 digits), set `JIRA_TICKET` to it (uppercased), and proceed to step 1.
   - If the user says "no ticket" / "don't have one" / "none" / similar: set `JIRA_TICKET` to empty and proceed to step 1.

1. **Generate a concise short name** (2-4 words) for the feature:
   - If `JIRA_TICKET` is set, exclude the ticket token from the description before extracting keywords — it must not appear in the short name
   - Analyze the feature description and extract the most meaningful keywords
   - Create a 2-4 word short name that captures the essence of the feature
   - Use action-noun format when possible (e.g., "add_user_auth", "fix_payment_bug")
   - Separate words with underscores (`_`), not hyphens — the only exception is a genuine hyphenated word (a compound conventionally hyphenated in English, e.g. "built-in", "cross-platform", "add-on"), which keeps its internal hyphen as a single word
   - Preserve technical terms and acronyms (OAuth2, API, JWT, etc.)
   - Keep it concise but descriptive enough to understand the feature at a glance
   - Examples:
     - "GEN-42 I want to add user authentication" → "user_auth"
     - "GEN-99 Implement OAuth2 integration for the API" → "oauth2_api_integration"
     - "Create a dashboard for analytics" (no ticket) → "analytics_dashboard"
     - "Fix payment processing timeout bug" (no ticket) → "fix_payment_timeout"
     - "GEN-456 Add a new built-in command" → "new_built-in_command" (hyphenated word "built-in" kept intact, still underscore-separated from the other words)

2. **Create the git branch and spec feature directory**:

   Specs live under the default `specs/` directory unless the user explicitly provides `SPECIFY_FEATURE_DIRECTORY`.

   **If the user explicitly provided `SPECIFY_FEATURE_DIRECTORY`** (e.g., via environment variable, argument, or configuration): use it as-is, skip the script call below, and instead `mkdir -p SPECIFY_FEATURE_DIRECTORY`, copy the resolved `spec-template` to `SPECIFY_FEATURE_DIRECTORY/spec.md`, and persist `{"feature_directory": "<resolved dir>"}` to `.specify/feature.json` directly. This override path intentionally bypasses branch-name-derived naming, so no git branch is created for it.

   **Otherwise (the common path)** — run `.specify/scripts/bash/create-new-feature.sh --json` to create the git branch, the spec feature directory, and `spec.md` in one step. Branch creation happens inside this script (a plain `git checkout -b`), not through a hook — there is no `.specify/extensions.yml` involved.

   ```bash
   .specify/scripts/bash/create-new-feature.sh --json --short-name "<short-name from step 1>" [--no-ticket] "<feature description>"
   ```

   - Omit `--no-ticket` when `JIRA_TICKET` was resolved in step 0/0b — the script extracts `GEN-XXXX` from the feature description itself and uses it as the branch/directory prefix (`<JIRA_TICKET>_<short-name>`, e.g. `GEN-42_user_auth` — the ticket number keeps its own internal dash, but the separator joining it to the short name, and joining words within the short name, is an underscore).
   - Pass `--no-ticket` when the user confirmed "no ticket" in step 0b. In that mode the script numbers the directory sequentially (`NNN_<short-name>`) unless you also pass `--timestamp`.
     - Check `.specify/init-options.json` for `feature_numbering` (preferred) or `branch_numbering` (deprecated, migration only — will be removed in a future release): if `"timestamp"`, pass `--timestamp`; if `"sequential"` or absent, pass neither flag (sequential is the script's default).
     - If `branch_numbering` was used (and `feature_numbering` was absent), emit a one-line warning: "⚠️ `branch_numbering` in init-options.json is deprecated. Rename to `feature_numbering`."
   - Pass `--allow-existing-branch` only if the user explicitly asked to resume/reuse an existing feature; otherwise let the script error out on a collision rather than silently overwriting one.

   Parse the script's JSON stdout for `BRANCH_NAME`, `SPEC_FILE`, `FEATURE_NUM`, `JIRA_TICKET`. Set `SPECIFY_FEATURE_DIRECTORY` to the directory containing `SPEC_FILE`. The script already persisted `.specify/feature.json` — do not write it again.

   If the script exits non-zero (e.g., the branch or directory already exists), surface its stderr message to the user verbatim and STOP. Do not fall back to manual `mkdir`/`git checkout` — a non-zero exit means something needs the user's attention (existing branch, dirty working tree, etc.), not a silent retry.

   **IMPORTANT**:
   - You must only create one feature per `/speckit.specify` invocation
   - In the script-driven path, the git branch name and the spec directory name are the same value (`BRANCH_NAME`); they diverge only under the explicit `SPECIFY_FEATURE_DIRECTORY` override above
   - Never re-implement the script's mkdir/cp/checkout/persist logic inline — always call the script itself so its git-branch, template-resolution, and `feature.json` behavior stay in one place

3b. **Gather cross-service context** (only if `JIRA_TICKET` is non-empty; skip this entire step if the user chose "no ticket"):

   This ports the `require-jira` Spec Kit extension's research pipeline, but
   runs inline as part of this command instead of through a separate hook —
   no `.specify/extensions.yml`, no harness state machine.

   a. **Verify Jira connectivity**: call `sesame_status`. Find the `jira` entry
      in the returned `services` list.
      - If Jira is missing or its status is not `connected`: STOP.
        Report: `BLOCKED: Jira is not connected via Sesame (status=<status>).
        Re-authenticate with: sesame auth jira — then retry /speckit.specify.`
        Do not proceed to spec writing.
      - Note the status of any other service (Slack, Confluence, GitHub,
        Google Drive, Miro, Loom, Gmail) for steps c/d below. A disconnected
        optional service only means its section is skipped or degraded — it
        never blocks the command.

   b. **Fetch the ticket, its parent/epic, and siblings** — spawn a subagent
      (Task tool, `subagent_type=general`) with this prompt:

      > Fetch full detail for Jira ticket `<JIRA_TICKET>` via
      > `sesame_jira_get_issue`. Capture key, summary, issue_type, status,
      > description, up to the last 5 comments, subtasks, labels, priority,
      > parent, assignee, created, updated. If this call fails (ticket not
      > found or no access), reply with exactly one line starting with
      > `BLOCKED:` explaining why, and do nothing else.
      >
      > If `parent` is set, fetch it too via `sesame_jira_get_issue`
      > (soft-fail: log and continue if this fails). If the parent was
      > fetched, find its siblings via `sesame_jira_search(query="parent =
      > <PARENT_KEY>", limit=50)`, excluding `<JIRA_TICKET>` itself (soft-fail
      > the same way; note if the result was capped at 50).
      >
      > Write a markdown report to
      > `<feature_dir>/context-files/fetch_parent_and_siblings.md` with sections:
      > `## Main Ticket` (metadata table + `### Description` + `### Recent
      > Comments (last 5)` + `### Sub-tasks` table), `## Parent / Epic`,
      > `## Sibling Tickets` (table, or "No sibling tickets found."). Then
      > reply with ONE paragraph summarizing what you found (ticket
      > type/status, parent, sibling count) — do not paste the full report
      > back into the conversation.

      If the subagent's reply starts with `BLOCKED:`, stop the entire
      `/speckit.specify` command and surface that message to the user.
      Otherwise, note its one-paragraph summary and continue.

   c. **Search Slack** (skip if Slack was reported disconnected in step a) —
      spawn a subagent with this prompt:

      > Read `<feature_dir>/context-files/fetch_parent_and_siblings.md` for
      > context. Devise up to 5 `sesame_slack_search` queries most likely to
      > surface discussion of this ticket (always include `<JIRA_TICKET>`
      > itself). Run them (count=50 each). Read the results, then optionally
      > run up to 2 more follow-up rounds (max 5 new queries in round 2, max
      > 3 in round 3) chasing new leads (people, related tickets, feature
      > names) — skip a round if the previous one returned 0 results or
      > surfaced no new terms. Never repeat a query already tried.
      >
      > Group matched messages by thread, ordered chronologically by each
      > thread's earliest match. Write
      > `<feature_dir>/context-files/slack_search.md` with one `##` subsection per
      > thread (channel name, earliest-match date/permalink, matched
      > messages only — non-matching replies are omitted). If nothing was
      > found, write "No Slack messages found. Queries tried: ...". Reply
      > with ONE paragraph summarizing message/thread counts and queries used.

      Never let this step block the command — always continue to step d
      regardless of outcome.

   d. **Follow links** — spawn a subagent with this prompt:

      > Read `<feature_dir>/context-files/fetch_parent_and_siblings.md` and
      > `<feature_dir>/context-files/slack_search.md` (if it exists). Scan both
      > for URLs to: Confluence pages, GitHub issues/PRs/files, Google
      > Docs/Sheets/Slides, Miro boards, Loom videos, Gmail messages.
      > Deduplicate. Cap at 5 distinct URLs per service and 20 total fetches.
      > Fetch each with the matching Sesame tool
      > (`sesame_confluence_get_page`, `sesame_github_get_issue` /
      > `sesame_github_get_pull_request` / `sesame_github_get_file_content`,
      > `sesame_google_get_doc` / `_sheet` / `_slides`, `sesame_miro_get_board`,
      > `sesame_loom_get_video`, `sesame_gmail_get_message`). Skip (soft-fail)
      > any URL that errors or matches no pattern.
      >
      > Write `<feature_dir>/context-files/follow_links.md` with one `##`
      > subsection per service that had a successful fetch (title/link + a
      > short excerpt per item). If nothing was found or fetched, write "No
      > linked resources were found or fetched." Reply with ONE paragraph
      > summarizing what was fetched, by service.

      Never let this step block the command.

   e. **Assemble `context-summary.md`**: run
      `.specify/scripts/bash/assemble-jira-context.sh "<feature_dir>"
      "<JIRA_TICKET>"`. This compiles the three research reports above into
      `<feature_dir>/context-summary.md` (ticket summary, description, comments,
      parent/epic, siblings, sub-tasks, Slack discussion, linked resources —
      sections that found nothing collapse to a one-line "none found").
      If the script exits non-zero, report the error but do not abort the
      command — spec writing can still proceed without `context-summary.md`.

   f. Read `<feature_dir>/context-summary.md` (if it was produced) before writing the
      spec in step 6 below, and ground the spec in it.

4. Load the resolved active `spec-template` file to understand required sections.

5. **IF EXISTS**: Load `.specify/memory/constitution.md` for project principles and governance constraints.

6. Follow this execution flow:
    1. Parse user description from arguments
       If empty: ERROR "No feature description provided"
    2. Extract key concepts from description
       Identify: actors, actions, data, constraints
    3. For unclear aspects:
       - Make informed guesses based on context and industry standards
       - Only mark with [NEEDS CLARIFICATION: specific question] if:
         - The choice significantly impacts feature scope or user experience
         - Multiple reasonable interpretations exist with different implications
         - No reasonable default exists
       - **LIMIT: Maximum 3 [NEEDS CLARIFICATION] markers total**
       - Prioritize clarifications by impact: scope > security/privacy > user experience > technical details
    4. Fill User Scenarios & Testing section
       If no clear user flow: ERROR "Cannot determine user scenarios"
    5. Generate Functional Requirements
       Each requirement must be testable
       Use reasonable defaults for unspecified details (document assumptions in Assumptions section)
    6. Define Success Criteria
       Create measurable, technology-agnostic outcomes
       Include both quantitative metrics (time, performance, volume) and qualitative measures (user satisfaction, task completion)
       Each criterion must be verifiable without implementation details
    7. Identify Key Entities (if data involved)
    8. Return: SUCCESS (spec ready for planning)

6. Write the specification to SPEC_FILE using the template structure, replacing placeholders with concrete details derived from the feature description (arguments) while preserving section order and headings.

7. **Specification Quality Validation**: After writing the initial spec, validate it against quality criteria:

   a. **Create Spec Quality Checklist**: Generate a checklist file at `SPECIFY_FEATURE_DIRECTORY/checklists/requirements.md` using the checklist template structure with these validation items:

      ```markdown
      # Specification Quality Checklist: [FEATURE NAME]

      **Purpose**: Validate specification completeness and quality before proceeding to planning
      **Created**: [DATE]
      **Feature**: [Link to spec.md]

      ## Content Quality

      - [ ] No implementation details (languages, frameworks, APIs)
      - [ ] Focused on user value and business needs
      - [ ] Written for non-technical stakeholders
      - [ ] All mandatory sections completed

      ## Requirement Completeness

      - [ ] No [NEEDS CLARIFICATION] markers remain
      - [ ] Requirements are testable and unambiguous
      - [ ] Success criteria are measurable
      - [ ] Success criteria are technology-agnostic (no implementation details)
      - [ ] All acceptance scenarios are defined
      - [ ] Edge cases are identified
      - [ ] Scope is clearly bounded
      - [ ] Dependencies and assumptions identified

      ## Feature Readiness

      - [ ] All functional requirements have clear acceptance criteria
      - [ ] User scenarios cover primary flows
      - [ ] Feature meets measurable outcomes defined in Success Criteria
      - [ ] No implementation details leak into specification

      ## Notes

      - Items marked incomplete require spec updates before `/speckit.clarify` or `/speckit.plan`
      ```

   b. **Run Validation Check**: Review the spec against each checklist item:
      - For each item, determine if it passes or fails
      - Document specific issues found (quote relevant spec sections)

   c. **Handle Validation Results**:

      - **If all items pass**: Mark checklist complete and proceed to the Mandatory Post-Execution Hooks section

      - **If items fail (excluding [NEEDS CLARIFICATION])**:
        1. List the failing items and specific issues
        2. Update the spec to address each issue
        3. Re-run validation until all items pass (max 3 iterations)
        4. If still failing after 3 iterations, document remaining issues in checklist notes and warn user

      - **If [NEEDS CLARIFICATION] markers remain**:
        1. Extract all [NEEDS CLARIFICATION: ...] markers from the spec
        2. **LIMIT CHECK**: If more than 3 markers exist, keep only the 3 most critical (by scope/security/UX impact) and make informed guesses for the rest
        3. For each clarification needed (max 3), present options to user in this format:

           ```markdown
           ## Question [N]: [Topic]

           **Context**: [Quote relevant spec section]

           **What we need to know**: [Specific question from NEEDS CLARIFICATION marker]

           **Suggested Answers**:

           | Option | Answer | Implications |
           |--------|--------|--------------|
           | A      | [First suggested answer] | [What this means for the feature] |
           | B      | [Second suggested answer] | [What this means for the feature] |
           | C      | [Third suggested answer] | [What this means for the feature] |
           | Custom | Provide your own answer | [Explain how to provide custom input] |

           **Your choice**: _[Wait for user response]_
           ```

        4. **CRITICAL - Table Formatting**: Ensure markdown tables are properly formatted:
           - Use consistent spacing with pipes aligned
           - Each cell should have spaces around content: `| Content |` not `|Content|`
           - Header separator must have at least 3 dashes: `|--------|`
           - Test that the table renders correctly in markdown preview
        5. Number questions sequentially (Q1, Q2, Q3 - max 3 total)
        6. Present all questions together before waiting for responses
        7. Wait for user to respond with their choices for all questions (e.g., "Q1: A, Q2: Custom - [details], Q3: B")
        8. Update the spec by replacing each [NEEDS CLARIFICATION] marker with the user's selected or provided answer
        9. Re-run validation after all clarifications are resolved

   d. **Update Checklist**: After each validation iteration, update the checklist file with current pass/fail status

## Mandatory Post-Execution Hooks

**You MUST complete this section before reporting completion to the user.**

Check if `.specify/extensions.yml` exists in the project root.
- If it does not exist, or no hooks are registered under `hooks.after_specify`, skip to the Completion Report.
- If it exists, read it and look for entries under the `hooks.after_specify` key.
- If the YAML cannot be parsed or is invalid, skip hook checking silently and continue to the Completion Report.
- Filter out hooks where `enabled` is explicitly `false`. Treat hooks without an `enabled` field as enabled by default.
- For each remaining hook, do **not** attempt to interpret or evaluate hook `condition` expressions:
  - If the hook has no `condition` field, or it is null/empty, treat the hook as executable
  - If the hook defines a non-empty `condition`, skip the hook and leave condition evaluation to the HookExecutor implementation
- For each executable hook, output the following based on its `optional` flag:
  - **Mandatory hook** (`optional: false`) — **You MUST emit `EXECUTE_COMMAND:` for each mandatory hook**:
    ```
    ## Extension Hooks

    **Automatic Hook**: {extension}
    Executing: `/{command}`
    EXECUTE_COMMAND: {command}
    ```
    After emitting the block above you MUST actually invoke the hook and wait for it to finish before continuing. Run it the same way you would run the command yourself in this agent/session (the invocation may differ from the literal `{command}` id shown above, e.g. a skills-mode agent runs it as `/skill:speckit-...` or `$speckit-...`). Emitting the block alone does not run the hook.
  - **Optional hook** (`optional: true`):
    ```
    ## Extension Hooks

    **Optional Hook**: {extension}
    Command: `/{command}`
    Description: {description}

    Prompt: {prompt}
    To execute: `/{command}`
    ```

## Completion Report

Report completion to the user with:
- `SPECIFY_FEATURE_DIRECTORY` — the feature directory path
- `SPEC_FILE` — the spec file path
- `JIRA_TICKET` and `context-summary.md` path — if a ticket was resolved and cross-service context was gathered (step 3b)
- Checklist results summary
- Readiness for the next phase (`/speckit.clarify` or `/speckit.plan`)

**NOTE:** Git branch creation, spec directory/file creation, and `.specify/feature.json` persistence are all handled directly by `.specify/scripts/bash/create-new-feature.sh` in step 2 — no hook, no `.specify/extensions.yml`.

## Quick Guidelines

- Focus on **WHAT** users need and **WHY**.
- Avoid HOW to implement (no tech stack, APIs, code structure).
- Written for business stakeholders, not developers.
- DO NOT create any checklists that are embedded in the spec. That will be a separate command.

### Section Requirements

- **Mandatory sections**: Must be completed for every feature
- **Optional sections**: Include only when relevant to the feature
- When a section doesn't apply, remove it entirely (don't leave as "N/A")

### For AI Generation

When creating this spec from a user prompt:

1. **Make informed guesses**: Use context, industry standards, and common patterns to fill gaps
2. **Document assumptions**: Record reasonable defaults in the Assumptions section
3. **Limit clarifications**: Maximum 3 [NEEDS CLARIFICATION] markers - use only for critical decisions that:
   - Significantly impact feature scope or user experience
   - Have multiple reasonable interpretations with different implications
   - Lack any reasonable default
4. **Prioritize clarifications**: scope > security/privacy > user experience > technical details
5. **Think like a tester**: Every vague requirement should fail the "testable and unambiguous" checklist item
6. **Common areas needing clarification** (only if no reasonable default exists):
   - Feature scope and boundaries (include/exclude specific use cases)
   - User types and permissions (if multiple conflicting interpretations possible)
   - Security/compliance requirements (when legally/financially significant)

**Examples of reasonable defaults** (don't ask about these):

- Data retention: Industry-standard practices for the domain
- Performance targets: Standard web/mobile app expectations unless specified
- Error handling: User-friendly messages with appropriate fallbacks
- Authentication method: Standard session-based or OAuth2 for web apps
- Integration patterns: Use project-appropriate patterns (REST/GraphQL for web services, function calls for libraries, CLI args for tools, etc.)

### Success Criteria Guidelines

Success criteria must be:

1. **Measurable**: Include specific metrics (time, percentage, count, rate)
2. **Technology-agnostic**: No mention of frameworks, languages, databases, or tools
3. **User-focused**: Describe outcomes from user/business perspective, not system internals
4. **Verifiable**: Can be tested/validated without knowing implementation details

**Good examples**:

- "Users can complete checkout in under 3 minutes"
- "System supports 10,000 concurrent users"
- "95% of searches return results in under 1 second"
- "Task completion rate improves by 40%"

**Bad examples** (implementation-focused):

- "API response time is under 200ms" (too technical, use "Users see results instantly")
- "Database can handle 1000 TPS" (implementation detail, use user-facing metric)
- "React components render efficiently" (framework-specific)
- "Redis cache hit rate above 80%" (technology-specific)

## Done When

- [ ] Specification written to `SPEC_FILE` and validated against quality checklist
- [ ] Extension hooks dispatched or skipped according to the rules in Mandatory Post-Execution Hooks above
- [ ] Completion reported to user with feature directory, spec file path, and checklist results
