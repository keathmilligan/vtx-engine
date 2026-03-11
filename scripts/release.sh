#!/usr/bin/env bash
# release.sh — bump version, sync lockfile, tag, and push to trigger the
# publish workflow.
#
# Usage: ./scripts/release.sh vX.Y.Z [--dry-run]
#
# The script will:
#   1. Validate the tag format (vX.Y.Z) and that the version is a valid
#      forward increment from the latest existing tag.
#   2. Confirm there are no uncommitted changes (unless --dry-run).
#   3. Update version in all versioned files (Cargo.toml, package.json,
#      tauri.conf.json).
#   4. Sync Cargo.lock via `cargo update --workspace`.
#   5. Commit, create a git tag, and push (unless --dry-run).

set -euo pipefail

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

die() { echo "error: $*" >&2; exit 1; }

usage() {
    cat <<'EOF'
Usage: ./scripts/release.sh vX.Y.Z [--dry-run]

Validates version increment, updates versioned files, commits changes,
pushes, creates the tag, and pushes the tag.

Options:
  --dry-run   Apply local file updates only (no git commands).
EOF
    exit 0
}

parse_semver() {
    # Returns "major minor patch" or empty on failure
    local v="${1#v}"
    if [[ "$v" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
        echo "${BASH_REMATCH[1]} ${BASH_REMATCH[2]} ${BASH_REMATCH[3]}"
    fi
}

valid_tag() {
    [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

# Returns 0 if $1 > $2 in semver ordering.
semver_gt() {
    local a_maj a_min a_pat b_maj b_min b_pat
    read -r a_maj a_min a_pat <<< "$(parse_semver "$1")"
    read -r b_maj b_min b_pat <<< "$(parse_semver "$2")"

    if   (( a_maj > b_maj )); then return 0
    elif (( a_maj < b_maj )); then return 1
    elif (( a_min > b_min )); then return 0
    elif (( a_min < b_min )); then return 1
    elif (( a_pat > b_pat )); then return 0
    else return 1
    fi
}

# Validates that $1 is a correct semver increment over $2.
# - Patch bump: same major.minor, patch incremented by any amount.
# - Minor bump: same major, minor incremented, patch must be 0.
# - Major bump: major incremented, minor and patch must be 0.
is_valid_increment() {
    local a_maj a_min a_pat b_maj b_min b_pat
    read -r a_maj a_min a_pat <<< "$(parse_semver "$1")"
    read -r b_maj b_min b_pat <<< "$(parse_semver "$2")"

    if (( a_maj == b_maj && a_min == b_min )); then
        (( a_pat > b_pat )) && return 0
    elif (( a_maj == b_maj && a_min > b_min )); then
        (( a_pat == 0 )) && return 0
    elif (( a_maj > b_maj )); then
        (( a_min == 0 && a_pat == 0 )) && return 0
    fi
    return 1
}

# Update a JSON file's top-level "version" field.
update_json_version() {
    local file="$1"
    node -e "
const fs = require('fs');
const data = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
if (!('version' in data)) { console.error('error: no version field in ' + process.argv[1]); process.exit(1); }
data.version = process.argv[2];
fs.writeFileSync(process.argv[1], JSON.stringify(data, null, 2) + '\n');
" "$file" "$VERSION"
    echo "  updated $file"
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

DRY_RUN=false
TAG=""

for arg in "$@"; do
    case "$arg" in
        -h|--help)  usage ;;
        --dry-run)  DRY_RUN=true ;;
        *)
            [[ -z "$TAG" ]] || die "unexpected argument: $arg"
            TAG="$arg"
            ;;
    esac
done

[[ -n "$TAG" ]] || die "usage: $0 vX.Y.Z [--dry-run]"
valid_tag "$TAG" || die "invalid version tag: $TAG (expected vX.Y.Z)"

VERSION="${TAG#v}"

# ---------------------------------------------------------------------------
# Repository root
# ---------------------------------------------------------------------------

REPO_ROOT="$(git rev-parse --show-toplevel)"

# ---------------------------------------------------------------------------
# Tag / version validation
# ---------------------------------------------------------------------------

# Check the tag doesn't already exist
EXISTING_TAG=$(git tag --list "$TAG")
[[ -z "$EXISTING_TAG" ]] || die "tag $TAG already exists"

# Get latest tag for increment validation
LATEST_TAG=$(git tag --list 'v*' --merged HEAD --sort=-v:refname | head -n1)

if [[ -n "$LATEST_TAG" ]]; then
    echo "Latest tag      : $LATEST_TAG"
    echo "New tag         : $TAG"

    semver_gt "$TAG" "$LATEST_TAG" \
        || die "$VERSION is not greater than $LATEST_TAG"
    is_valid_increment "$TAG" "$LATEST_TAG" \
        || die "$VERSION is not a valid increment of $LATEST_TAG"
else
    echo "No previous tags found — treating as first release."
    echo "New tag         : $TAG"
fi

# ---------------------------------------------------------------------------
# Working tree must be clean (unless --dry-run)
# ---------------------------------------------------------------------------

if [[ "$DRY_RUN" == false ]]; then
    DIRTY=$(git status --porcelain)
    if [[ -n "$DIRTY" ]]; then
        die "working tree is not clean — commit or stash changes first"
    fi
fi

# ---------------------------------------------------------------------------
# Update versioned files
# ---------------------------------------------------------------------------

echo "Updating versions to $VERSION..."

# Workspace Cargo.toml (the authoritative version for Rust crates)
CARGO_TOML="$REPO_ROOT/Cargo.toml"
sed -i "0,/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/s//version = \"$VERSION\"/" "$CARGO_TOML"
# Verify the replacement took
UPDATED=$(grep -m1 '^version = ' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ "$UPDATED" == "$VERSION" ]] \
    || die "failed to update version in $CARGO_TOML (got '$UPDATED')"
echo "  updated $CARGO_TOML"

# package.json / tauri.conf.json files with a version field
update_json_version "$REPO_ROOT/packages/vtx-viz/package.json"
update_json_version "$REPO_ROOT/apps/vtx-demo/package.json"
update_json_version "$REPO_ROOT/apps/vtx-demo/src-tauri/tauri.conf.json"

# ---------------------------------------------------------------------------
# Sync Cargo.lock
# ---------------------------------------------------------------------------

echo "Syncing Cargo.lock..."
(cd "$REPO_ROOT" && cargo update --workspace)

# ---------------------------------------------------------------------------
# Dry-run exit
# ---------------------------------------------------------------------------

if [[ "$DRY_RUN" == true ]]; then
    echo ""
    echo "Dry run complete. Local files updated; no git commands executed."
    exit 0
fi

# ---------------------------------------------------------------------------
# Commit, tag, push
# ---------------------------------------------------------------------------

echo "Committing version bump..."
git add \
    "$REPO_ROOT/Cargo.toml" \
    "$REPO_ROOT/Cargo.lock" \
    "$REPO_ROOT/packages/vtx-viz/package.json" \
    "$REPO_ROOT/apps/vtx-demo/package.json" \
    "$REPO_ROOT/apps/vtx-demo/src-tauri/tauri.conf.json"

STAGED=$(git diff --cached --name-only)
if [[ -z "$STAGED" ]]; then
    die "no changes staged for commit — aborting release"
fi

git commit -m "chore: release $TAG"

echo "Pushing commit..."
git push

echo "Creating and pushing tag $TAG..."
git tag "$TAG"
git push origin "$TAG"

echo ""
echo "Release $TAG prepared and pushed."
