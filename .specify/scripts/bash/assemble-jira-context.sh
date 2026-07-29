#!/usr/bin/env bash
# assemble-jira-context.sh
#
# Assembles <feature_dir>/context-summary.md from the three research reports
# written by the Jira/Slack/link-following subagent steps in
# speckit.specify.md:
#   context-files/fetch_parent_and_siblings.md
#   context-files/slack_search.md
#   context-files/follow_links.md
#
# Ported from the require-jira Spec Kit extension's assemble-context.sh,
# decoupled from its feature.json/harness state machine — this is a plain
# stdin/stdout script invoked directly by the /speckit.specify command.
#
# Usage: assemble-jira-context.sh <feature_dir> <ticket_key>

set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: $0 <feature_dir> <ticket_key>" >&2
    exit 1
fi

FEATURE_DIR="$1"
TICKET_KEY="$2"

TICKET_REPORT="${FEATURE_DIR}/context-files/fetch_parent_and_siblings.md"
SLACK_REPORT="${FEATURE_DIR}/context-files/slack_search.md"
LINKS_REPORT="${FEATURE_DIR}/context-files/follow_links.md"
CONTEXT_MD="${FEATURE_DIR}/context-summary.md"

if [[ ! -f "$TICKET_REPORT" ]]; then
    echo "ERROR: ticket report not found: $TICKET_REPORT" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# extract_section <file> <heading>
#
# Prints all lines between <heading> and the next heading of equal or higher
# level (i.e. with <= the same number of leading # chars). Strips leading
# blank lines from the output.
# ---------------------------------------------------------------------------
extract_section() {
    local file="$1"
    local heading="$2"
    awk -v h="$heading" '
        BEGIN {
            hashes = h
            gsub(/[^#].*$/, "", hashes)
            lvl = length(hashes)
        }
        $0 == h { found = 1; next }
        found {
            if (match($0, /^#+/) && RLENGTH <= lvl) exit
            print
        }
    ' "$file" | sed '/./,$!d'
}

# ---------------------------------------------------------------------------
# Assemble context-summary.md
# ---------------------------------------------------------------------------
{
    echo "# Context for ${TICKET_KEY}"
    echo ""

    # Metadata table — the | lines from ## Main Ticket, before ### Description
    MAIN_BODY=$(extract_section "$TICKET_REPORT" "## Main Ticket")
    SUMMARY_LINE=$(echo "$MAIN_BODY" | grep -m1 '^\*\*' || true)
    META_TABLE=$(echo "$MAIN_BODY" | awk '/^###/{exit} /^\|/{print}' || true)

    if [[ -n "$META_TABLE" ]]; then
        echo "$META_TABLE"
        echo ""
    fi

    # Summary
    echo "## Summary"
    echo ""
    if [[ -n "$SUMMARY_LINE" ]]; then
        echo "$SUMMARY_LINE"
    else
        echo "_No summary available._"
    fi
    echo ""

    # Description
    DESCRIPTION=$(extract_section "$TICKET_REPORT" "### Description")
    echo "## Description"
    echo ""
    if [[ -n "$DESCRIPTION" ]]; then
        echo "$DESCRIPTION"
    else
        echo "_No description._"
    fi
    echo ""

    # Recent Comments
    COMMENTS=$(extract_section "$TICKET_REPORT" "### Recent Comments (last 5)")
    echo "## Recent Comments"
    echo ""
    if [[ -n "$COMMENTS" ]]; then
        echo "$COMMENTS"
    else
        echo "No comments on this ticket."
    fi
    echo ""

    # Parent / Epic
    PARENT=$(extract_section "$TICKET_REPORT" "## Parent / Epic")
    echo "## Parent / Epic"
    echo ""
    if [[ -n "$PARENT" ]]; then
        echo "$PARENT"
    else
        echo "No parent ticket linked."
    fi
    echo ""

    # Sibling Tickets
    SIBLINGS=$(extract_section "$TICKET_REPORT" "## Sibling Tickets")
    echo "## Sibling Tickets"
    echo ""
    if [[ -n "$SIBLINGS" ]]; then
        echo "$SIBLINGS"
    else
        echo "No sibling tickets found."
    fi
    echo ""

    # Sub-tasks
    SUBTASKS=$(extract_section "$TICKET_REPORT" "### Sub-tasks")
    echo "## Sub-tasks"
    echo ""
    if [[ -n "$SUBTASKS" ]]; then
        echo "$SUBTASKS"
    else
        echo "No sub-tasks."
    fi
    echo ""

    # Slack Discussion
    if [[ -f "$SLACK_REPORT" ]]; then
        SLACK_BODY=$(awk 'NR > 1' "$SLACK_REPORT" | sed '/./,$!d')
        if [[ -z "$SLACK_BODY" ]] \
            || echo "$SLACK_BODY" | grep -qiE 'slack was unavailable|no slack messages found'; then
            echo "## Slack Discussion"
            echo ""
            echo "No Slack discussion found for this ticket."
            echo ""
        else
            echo "## Slack Discussion"
            echo ""
            echo "$SLACK_BODY"
            echo ""
        fi
    else
        echo "## Slack Discussion"
        echo ""
        echo "No Slack discussion found for this ticket."
        echo ""
    fi

    # Linked Resources — omit entire section if empty or absent
    if [[ -f "$LINKS_REPORT" ]]; then
        LINKS_BODY=$(awk 'NR > 1' "$LINKS_REPORT" | sed '/./,$!d')
        if [[ -n "$LINKS_BODY" ]] \
            && ! echo "$LINKS_BODY" | grep -qiE 'no linked resources were found'; then
            echo "## Linked Resources"
            echo ""
            echo "$LINKS_BODY"
            echo ""
        fi
    fi

} > "$CONTEXT_MD"

echo "context-summary.md assembled: $CONTEXT_MD"
