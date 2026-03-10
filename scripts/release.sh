#!/usr/bin/env bash
# release.sh — bump version, tag, and push to trigger the publish workflow.
#
# Usage: ./scripts/release.sh <new-version>
#   e.g. ./scripts/release.sh 0.1.2
#
# The script will:
#   1. Validate the new version is a valid semver and a forward increment.
#   2. Confirm there are no uncommitted changes.
#   3. Update the version in Cargo.toml (workspace root).
#   4. Commit the change.
#   5. Create an annotated git tag vX.Y.Z.
#   6. Push the commit and tag to origin.

set -euo pipefail

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

die() { echo "error: $*" >&2; exit 1; }

semver_parts() {
    # Strips optional leading 'v', then splits on '.'
    local v="${1#v}"
    IFS='.' read -r major minor patch <<< "$v"
    echo "$major $minor $patch"
}

valid_semver() {
    [[ "$1" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

# Compare two bare semver strings (no 'v' prefix). Returns 0 if $1 > $2.
semver_gt() {
    local a_maj a_min a_pat b_maj b_min b_pat
    read -r a_maj a_min a_pat <<< "$(semver_parts "$1")"
    read -r b_maj b_min b_pat <<< "$(semver_parts "$2")"

    if   (( a_maj > b_maj )); then return 0
    elif (( a_maj < b_maj )); then return 1
    elif (( a_min > b_min )); then return 0
    elif (( a_min < b_min )); then return 1
    elif (( a_pat > b_pat )); then return 0
    else return 1
    fi
}

# ---------------------------------------------------------------------------
# Argument validation
# ---------------------------------------------------------------------------

[[ $# -eq 1 ]] || die "usage: $0 <new-version>  (e.g. $0 0.1.2)"

NEW_VERSION="${1#v}"   # strip any leading 'v' the user may have typed

valid_semver "$NEW_VERSION" || die "'$NEW_VERSION' is not a valid semver (expected X.Y.Z)"

CARGO_TOML="$(git rev-parse --show-toplevel)/Cargo.toml"
[[ -f "$CARGO_TOML" ]] || die "Cargo.toml not found at $CARGO_TOML"

# Read current workspace version
CURRENT_VERSION=$(grep -m1 '^version = ' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ -n "$CURRENT_VERSION" ]] || die "Could not read current version from Cargo.toml"

echo "Current version : $CURRENT_VERSION"
echo "New version     : $NEW_VERSION"

# Verify the new version is strictly greater than the current one
semver_gt "$NEW_VERSION" "$CURRENT_VERSION" \
    || die "'$NEW_VERSION' is not a forward increment from '$CURRENT_VERSION'"

TAG="v${NEW_VERSION}"

# Verify the tag does not already exist locally or on remote
git fetch --tags --quiet
git rev-parse "$TAG" &>/dev/null && die "Tag '$TAG' already exists"

# ---------------------------------------------------------------------------
# Working tree must be clean
# ---------------------------------------------------------------------------

if ! git diff --quiet || ! git diff --cached --quiet; then
    die "Working tree has uncommitted changes — commit or stash them first"
fi

# ---------------------------------------------------------------------------
# Update Cargo.toml
# ---------------------------------------------------------------------------

echo "Updating Cargo.toml..."
# Replace the first occurrence of  version = "X.Y.Z"  (the workspace version line)
sed -i '' "0,/^version = \"${CURRENT_VERSION}\"/s//version = \"${NEW_VERSION}\"/" "$CARGO_TOML"

# Verify the replacement took
UPDATED_VERSION=$(grep -m1 '^version = ' "$CARGO_TOML" | sed 's/version = "\(.*\)"/\1/')
[[ "$UPDATED_VERSION" == "$NEW_VERSION" ]] \
    || die "Failed to update version in Cargo.toml (got '$UPDATED_VERSION')"

# ---------------------------------------------------------------------------
# Commit, tag, push
# ---------------------------------------------------------------------------

echo "Committing version bump..."
git add "$CARGO_TOML"
git commit -m "chore: bump version to ${NEW_VERSION}"

echo "Creating tag $TAG..."
git tag -a "$TAG" -m "Release ${NEW_VERSION}"

echo "Pushing commit and tag to origin..."
git push origin HEAD
git push origin "$TAG"

echo ""
echo "Released $TAG — the publish workflow will start shortly."
