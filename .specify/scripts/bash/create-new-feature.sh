#!/usr/bin/env bash

set -e

JSON_MODE=false
DRY_RUN=false
ALLOW_EXISTING=false
SHORT_NAME=""
BRANCH_NUMBER=""
USE_TIMESTAMP=false
NO_TICKET=false
ARGS=()
i=1
while [ $i -le $# ]; do
    arg="${!i}"
    case "$arg" in
        --json)
            JSON_MODE=true
            ;;
        --dry-run)
            DRY_RUN=true
            ;;
        --allow-existing-branch)
            ALLOW_EXISTING=true
            ;;
        --short-name)
            if [ $((i + 1)) -gt $# ]; then
                echo 'Error: --short-name requires a value' >&2
                exit 1
            fi
            i=$((i + 1))
            next_arg="${!i}"
            # Check if the next argument is another option (starts with --)
            if [[ "$next_arg" == --* ]]; then
                echo 'Error: --short-name requires a value' >&2
                exit 1
            fi
            SHORT_NAME="$next_arg"
            ;;
        --number)
            if [ $((i + 1)) -gt $# ]; then
                echo 'Error: --number requires a value' >&2
                exit 1
            fi
            i=$((i + 1))
            next_arg="${!i}"
            if [[ "$next_arg" == --* ]]; then
                echo 'Error: --number requires a value' >&2
                exit 1
            fi
            BRANCH_NUMBER="$next_arg"
            ;;
        --timestamp)
            USE_TIMESTAMP=true
            ;;
        --no-ticket)
            NO_TICKET=true
            ;;
        --help|-h)
            echo "Usage: $0 [--json] [--dry-run] [--allow-existing-branch] [--short-name <name>] [--number N] [--timestamp] [--no-ticket] <feature_description>"
            echo ""
            echo "The feature description should contain a JIRA ticket in the format GEN-XXXX"
            echo "(where XXXX is 1-5 digits). Use --no-ticket if you don't have a ticket."
            echo ""
            echo "Options:"
            echo "  --json              Output in JSON format"
            echo "  --dry-run           Compute feature name and paths without creating directories or files"
            echo "  --allow-existing-branch  Reuse an existing feature directory if it already exists"
            echo "  --short-name <name> Provide a custom short name (2-4 words) for the feature"
            echo "  --number N          Specify branch number manually (no-ticket mode only)"
            echo "  --timestamp         Use timestamp prefix instead of sequential numbering (no-ticket mode only)"
            echo "  --no-ticket         Create feature without a JIRA ticket prefix"
            echo "  --help, -h          Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0 'GEN-42 Add user authentication system'"
            echo "  $0 'GEN-42 Implement OAuth2 integration' --short-name 'oauth2_api'"
            echo "  $0 --no-ticket 'Add user authentication system' --short-name 'user_auth'"
            echo ""
            echo "Directory naming formats (words are underscore-separated; a genuine"
            echo "hyphenated English compound, e.g. 'built-in', keeps its internal dash):"
            echo "  With ticket:    GEN-42_user_authentication"
            echo "  Without ticket: 003_user_authentication (or timestamp-prefixed)"
            exit 0
            ;;
        *)
            ARGS+=("$arg")
            ;;
    esac
    i=$((i + 1))
done

FEATURE_DESCRIPTION="${ARGS[*]}"
if [ -z "$FEATURE_DESCRIPTION" ]; then
    echo "Usage: $0 [--json] [--dry-run] [--allow-existing-branch] [--short-name <name>] [--number N] [--timestamp] [--no-ticket] <feature_description>" >&2
    exit 1
fi

# Trim whitespace and validate description is not empty (e.g., user passed only whitespace)
FEATURE_DESCRIPTION=$(echo "$FEATURE_DESCRIPTION" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
if [ -z "$FEATURE_DESCRIPTION" ]; then
    echo "Error: Feature description cannot be empty or contain only whitespace" >&2
    exit 1
fi

# Function to extract exactly one JIRA ticket (GEN-XXXX, 1-5 digits) from the description
extract_jira_ticket() {
    local description="$1"
    local tickets
    tickets=$(printf '%s' "$description" | grep -oiE 'GEN-[0-9]{1,5}' | tr '[:lower:]' '[:upper:]' | sort -u)

    local count
    count=$(printf '%s\n' "$tickets" | grep -c 'GEN-' 2>/dev/null || true)
    [ -z "$count" ] && count=0

    if [ "$count" -eq 0 ]; then
        echo "Error: No JIRA ticket found in description." >&2
        echo "Either include a JIRA ticket (e.g., GEN-42) or use --no-ticket." >&2
        exit 1
    fi

    if [ "$count" -gt 1 ]; then
        echo "Error: Multiple JIRA tickets found in description: $(echo "$tickets" | tr '\n' ' ')" >&2
        echo "Please provide exactly one JIRA ticket per feature." >&2
        exit 1
    fi

    printf '%s' "$tickets"
}

# Extract JIRA ticket from description (unless --no-ticket flag is set), and strip
# the ticket token out of the description used for short-name generation.
JIRA_TICKET=""
DESC_FOR_NAMING="$FEATURE_DESCRIPTION"
if [ "$NO_TICKET" = false ]; then
    JIRA_TICKET=$(extract_jira_ticket "$FEATURE_DESCRIPTION")
    DESC_FOR_NAMING=$(printf '%s' "$FEATURE_DESCRIPTION" | sed -E 's/GEN-[0-9]{1,5}//gi')
fi

# Function to get highest number from specs directory
get_highest_from_specs() {
    local specs_dir="$1"
    local highest=0

    if [ -d "$specs_dir" ]; then
        for dir in "$specs_dir"/*; do
            [ -d "$dir" ] || continue
            dirname=$(basename "$dir")
            # Match sequential prefixes (>=3 digits), but skip timestamp dirs.
            if echo "$dirname" | grep -Eq '^[0-9]{3,}_' && ! echo "$dirname" | grep -Eq '^[0-9]{8}-[0-9]{6}_'; then
                number=$(echo "$dirname" | grep -Eo '^[0-9]+')
                number=$((10#$number))
                if [ "$number" -gt "$highest" ]; then
                    highest=$number
                fi
            fi
        done
    fi

    echo "$highest"
}

# Function to clean and format a branch name.
# Words join with underscores; a genuine hyphenated English compound already
# in the input (e.g. "built-in") keeps its internal hyphen as one word.
clean_branch_name() {
    local name="$1"
    printf '%s' "$name" \
        | tr '[:upper:]' '[:lower:]' \
        | sed -E 's/[^a-z0-9-]+/_/g' \
        | sed -E 's/^-+//; s/-+$//' \
        | sed -E 's/-_/_/g; s/_-/_/g' \
        | sed -E 's/-+/-/g; s/_+/_/g' \
        | sed -E 's/^_+//; s/_+$//'
}

# Resolve repository root using common.sh functions which prioritize .specify
SCRIPT_DIR="$(CDPATH="" cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

REPO_ROOT=$(get_repo_root) || exit 1

cd "$REPO_ROOT"

# Detect whether we're inside a git working tree. Branch creation below is
# skipped (with a warning) when git is unavailable, matching upstream Spec
# Kit's git extension (extensions/git/scripts/bash/create-new-feature-branch.sh).
if git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    HAS_GIT=true
else
    HAS_GIT=false
fi

SPECS_DIR="$REPO_ROOT/specs"
if [ "$DRY_RUN" != true ]; then
    mkdir -p "$SPECS_DIR"
fi

# Function to generate branch name with stop word filtering and length filtering
generate_branch_name() {
    local description="$1"

    # Common stop words to filter out
    local stop_words="^(i|a|an|the|to|for|of|in|on|at|by|with|from|is|are|was|were|be|been|being|have|has|had|do|does|did|will|would|should|could|can|may|might|must|shall|this|that|these|those|my|your|our|their|want|need|add|get|set)$"

    # Split into words; hyphens survive so a compound like "built-in" stays one word.
    local clean_name=$(printf '%s' "$description" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9-]+/ /g')

    # Filter words: remove stop words and words shorter than 3 chars (unless they're uppercase acronyms in original)
    local meaningful_words=()
    for word in $clean_name; do
        # Drop leading/trailing hyphens from punctuation, e.g. "(built-in)" -> "built-in".
        word=$(printf '%s' "$word" | sed -E 's/^-+//; s/-+$//')
        # Skip empty words
        [ -z "$word" ] && continue

        # Keep words that are NOT stop words AND (length >= 3 OR are potential acronyms)
        if ! echo "$word" | grep -qiE "$stop_words"; then
            if [ ${#word} -ge 3 ]; then
                meaningful_words+=("$word")
            # Keep short words that appear as an uppercase acronym in the original.
            # Uppercase via tr and match with grep -w (both portable) rather than
            # bash's 4+ "^^" case expansion (breaks on macOS bash 3.2) and \b (non-POSIX).
            elif printf '%s' "$description" | grep -qw -- "$(printf '%s' "$word" | tr '[:lower:]' '[:upper:]')"; then
                meaningful_words+=("$word")
            fi
        fi
    done

    # Use first 3-4 meaningful words, underscore-joined.
    if [ ${#meaningful_words[@]} -gt 0 ]; then
        local max_words=3
        if [ ${#meaningful_words[@]} -eq 4 ]; then max_words=4; fi

        local result=""
        local count=0
        for word in "${meaningful_words[@]}"; do
            if [ $count -ge $max_words ]; then break; fi
            if [ -n "$result" ]; then result="${result}_"; fi
            result="$result$word"
            count=$((count + 1))
        done
        echo "$result"
    else
        # Fallback to original logic if no meaningful words found
        local cleaned=$(clean_branch_name "$description")
        echo "$cleaned" | tr '_' '\n' | grep -v '^$' | head -3 | tr '\n' '_' | sed 's/_$//'
    fi
}

# Generate branch name
if [ -n "$SHORT_NAME" ]; then
    # Use provided short name, just clean it up
    BRANCH_SUFFIX=$(clean_branch_name "$SHORT_NAME")
else
    # Generate from description (with the JIRA ticket, if any, stripped out) with smart filtering
    BRANCH_SUFFIX=$(generate_branch_name "$DESC_FOR_NAMING")
fi

# Warn if --number and --timestamp are both specified
if [ "$USE_TIMESTAMP" = true ] && [ -n "$BRANCH_NUMBER" ]; then
    >&2 echo "[specify] Warning: --number is ignored when --timestamp is used"
    BRANCH_NUMBER=""
fi

# Determine branch prefix
if [ -n "$JIRA_TICKET" ]; then
    if [ "$USE_TIMESTAMP" = true ] || [ -n "$BRANCH_NUMBER" ]; then
        >&2 echo "[specify] Warning: --number/--timestamp are ignored when a JIRA ticket is present"
    fi
    FEATURE_NUM="$JIRA_TICKET"
    BRANCH_NAME="${JIRA_TICKET}_${BRANCH_SUFFIX}"
elif [ "$USE_TIMESTAMP" = true ]; then
    FEATURE_NUM=$(date +%Y%m%d-%H%M%S)
    BRANCH_NAME="${FEATURE_NUM}_${BRANCH_SUFFIX}"
else
    # Determine branch number from existing feature directories
    if [ -z "$BRANCH_NUMBER" ]; then
        HIGHEST=$(get_highest_from_specs "$SPECS_DIR")
        BRANCH_NUMBER=$((HIGHEST + 1))
    fi

    # Force base-10 interpretation to prevent octal conversion (e.g., 010 → 8 in octal, but should be 10 in decimal)
    FEATURE_NUM=$(printf "%03d" "$((10#$BRANCH_NUMBER))")
    BRANCH_NAME="${FEATURE_NUM}_${BRANCH_SUFFIX}"
fi

# GitHub enforces a 244-byte limit on branch names
# Validate and truncate if necessary
MAX_BRANCH_LENGTH=244
if [ ${#BRANCH_NAME} -gt $MAX_BRANCH_LENGTH ]; then
    # Calculate how much we need to trim from suffix
    # Account for prefix length: timestamp (15) + underscore (1) = 16, or sequential (3) + underscore (1) = 4
    PREFIX_LENGTH=$(( ${#FEATURE_NUM} + 1 ))
    MAX_SUFFIX_LENGTH=$((MAX_BRANCH_LENGTH - PREFIX_LENGTH))

    # Truncate suffix at word boundary if possible
    TRUNCATED_SUFFIX=$(echo "$BRANCH_SUFFIX" | cut -c1-$MAX_SUFFIX_LENGTH)
    # Remove trailing separator if truncation created one
    TRUNCATED_SUFFIX=$(echo "$TRUNCATED_SUFFIX" | sed -E 's/[-_]$//')

    ORIGINAL_BRANCH_NAME="$BRANCH_NAME"
    BRANCH_NAME="${FEATURE_NUM}_${TRUNCATED_SUFFIX}"

    >&2 echo "[specify] Warning: Branch name exceeded GitHub's 244-byte limit"
    >&2 echo "[specify] Original: $ORIGINAL_BRANCH_NAME (${#ORIGINAL_BRANCH_NAME} bytes)"
    >&2 echo "[specify] Truncated to: $BRANCH_NAME (${#BRANCH_NAME} bytes)"
fi

FEATURE_DIR="$SPECS_DIR/$BRANCH_NAME"
SPEC_FILE="$FEATURE_DIR/spec.md"

# Create (or, with --allow-existing-branch, switch to) the git branch for this
# feature. Ported directly from upstream Spec Kit's git extension
# (extensions/git/scripts/bash/create-new-feature-branch.sh) so branch
# creation is deterministic, always-on behavior instead of an optional hook
# dispatched through .specify/extensions.yml.
if [ "$DRY_RUN" != true ]; then
    if [ "$HAS_GIT" = true ]; then
        if ! branch_create_error=$(git checkout -q -b "$BRANCH_NAME" 2>&1); then
            current_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
            if git branch --list "$BRANCH_NAME" | grep -q .; then
                if [ "$ALLOW_EXISTING" = true ]; then
                    if [ "$current_branch" != "$BRANCH_NAME" ] && ! switch_branch_error=$(git checkout -q "$BRANCH_NAME" 2>&1); then
                        >&2 echo "Error: Failed to switch to existing branch '$BRANCH_NAME'. Please resolve any local changes or conflicts and try again."
                        [ -n "$switch_branch_error" ] && >&2 printf '%s\n' "$switch_branch_error"
                        exit 1
                    fi
                elif [ "$USE_TIMESTAMP" = true ]; then
                    >&2 echo "Error: Branch '$BRANCH_NAME' already exists. Rerun to get a new timestamp or use a different --short-name."
                    exit 1
                else
                    >&2 echo "Error: Branch '$BRANCH_NAME' already exists. Please use a different feature name or specify a different number with --number."
                    exit 1
                fi
            else
                >&2 echo "Error: Failed to create git branch '$BRANCH_NAME'."
                if [ -n "$branch_create_error" ]; then
                    >&2 printf '%s\n' "$branch_create_error"
                else
                    >&2 echo "Please check your git configuration and try again."
                fi
                exit 1
            fi
        fi
    else
        >&2 echo "[specify] Warning: Git repository not detected; skipped branch creation for $BRANCH_NAME"
    fi
fi

if [ "$DRY_RUN" != true ]; then
    if [ -d "$FEATURE_DIR" ] && [ "$ALLOW_EXISTING" != true ]; then
        if [ "$USE_TIMESTAMP" = true ]; then
            >&2 echo "Error: Feature directory '$FEATURE_DIR' already exists. Rerun to get a new timestamp or use a different --short-name."
        else
            >&2 echo "Error: Feature directory '$FEATURE_DIR' already exists. Please use a different feature name or specify a different number with --number."
        fi
        exit 1
    fi

    mkdir -p "$FEATURE_DIR"

    if [ ! -f "$SPEC_FILE" ]; then
        TEMPLATE=$(resolve_template "spec-template" "$REPO_ROOT") || true
        if [ -n "$TEMPLATE" ] && [ -f "$TEMPLATE" ]; then
            cp "$TEMPLATE" "$SPEC_FILE"
        else
            echo "Warning: Spec template not found; created empty spec file" >&2
            touch "$SPEC_FILE"
        fi
    fi

    # Persist to .specify/feature.json so downstream commands can find the feature
    _persist_feature_json "$REPO_ROOT" "$FEATURE_DIR"

    # Inform the user how to set feature state in their own shell
    printf '# To persist: export SPECIFY_FEATURE=%q\n' "$BRANCH_NAME" >&2
    printf '#              export SPECIFY_FEATURE_DIRECTORY=%q\n' "$FEATURE_DIR" >&2
fi

if $JSON_MODE; then
    if command -v jq >/dev/null 2>&1; then
        if [ "$DRY_RUN" = true ]; then
            jq -cn \
                --arg branch_name "$BRANCH_NAME" \
                --arg spec_file "$SPEC_FILE" \
                --arg feature_num "$FEATURE_NUM" \
                --arg jira_ticket "$JIRA_TICKET" \
                '{BRANCH_NAME:$branch_name,SPEC_FILE:$spec_file,FEATURE_NUM:$feature_num,JIRA_TICKET:$jira_ticket,DRY_RUN:true}'
        else
            jq -cn \
                --arg branch_name "$BRANCH_NAME" \
                --arg spec_file "$SPEC_FILE" \
                --arg feature_num "$FEATURE_NUM" \
                --arg jira_ticket "$JIRA_TICKET" \
                '{BRANCH_NAME:$branch_name,SPEC_FILE:$spec_file,FEATURE_NUM:$feature_num,JIRA_TICKET:$jira_ticket}'
        fi
    else
        if [ "$DRY_RUN" = true ]; then
            printf '{"BRANCH_NAME":"%s","SPEC_FILE":"%s","FEATURE_NUM":"%s","JIRA_TICKET":"%s","DRY_RUN":true}\n' "$(json_escape "$BRANCH_NAME")" "$(json_escape "$SPEC_FILE")" "$(json_escape "$FEATURE_NUM")" "$(json_escape "$JIRA_TICKET")"
        else
            printf '{"BRANCH_NAME":"%s","SPEC_FILE":"%s","FEATURE_NUM":"%s","JIRA_TICKET":"%s"}\n' "$(json_escape "$BRANCH_NAME")" "$(json_escape "$SPEC_FILE")" "$(json_escape "$FEATURE_NUM")" "$(json_escape "$JIRA_TICKET")"
        fi
    fi
else
    echo "BRANCH_NAME: $BRANCH_NAME"
    echo "SPEC_FILE: $SPEC_FILE"
    echo "FEATURE_NUM: $FEATURE_NUM"
    echo "JIRA_TICKET: $JIRA_TICKET"
    if [ "$DRY_RUN" != true ]; then
        printf '# To persist in your shell: export SPECIFY_FEATURE=%q\n' "$BRANCH_NAME"
        printf '#                           export SPECIFY_FEATURE_DIRECTORY=%q\n' "$FEATURE_DIR"
    fi
fi
