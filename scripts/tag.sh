#!/usr/bin/env bash
# Tags the next version of Darkroom. Bumps the workspace version in Cargo.toml (and Cargo.lock),
# runs the tests and clippy, commits "Release vX.Y.Z" and tags it. With --push it pushes the
# commit and the tag together, which starts the release workflow: every platform is built and
# attached to a GitHub release of that tag.
#
# The next version counts up from the newer of Cargo.toml's version and the latest vX.Y.Z tag,
# so neither is ever reused.
#
# Usage: scripts/tag.sh [patch|minor|major] [--push] [--dry-run]
#   patch      0.1.2 -> 0.1.3 (the default)
#   minor      0.1.2 -> 0.2.0
#   major      0.1.2 -> 1.0.0
#   --push     push the commit and tag to origin
#   --dry-run  say what would happen and change nothing
set -euo pipefail
cd "$(dirname "$0")/.."

usage() { sed -n '2,/^set /p' "$0" | sed '$d; s/^# \{0,1\}//'; }

part=patch push=false dry=false
for arg in "$@"; do
    case $arg in
        patch | minor | major) part=$arg ;;
        --push) push=true ;;
        --dry-run) dry=true ;;
        -h | --help) usage; exit 0 ;;
        *) echo "tag.sh: unknown argument '$arg'" >&2; usage >&2; exit 2 ;;
    esac
done

fail() { echo "tag.sh: $*" >&2; exit 1; }

[ -z "$(git status --porcelain)" ] || fail "the working tree has changes; commit or stash them first"
branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = main ] || fail "on '$branch'; releases are tagged on main"

in_cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
latest_tag=$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -1)
base=$(printf '%s\n' "$in_cargo" "${latest_tag#v}" | grep . | sort -V | tail -1)

IFS=. read -r major minor patch <<< "$base"
case $part in
    major) next="$((major + 1)).0.0" ;;
    minor) next="$major.$((minor + 1)).0" ;;
    patch) next="$major.$minor.$((patch + 1))" ;;
esac
tag="v$next"
git rev-parse -q --verify "refs/tags/$tag" > /dev/null && fail "$tag already exists"

echo "Cargo.toml $in_cargo, latest tag ${latest_tag:-none}: next is $tag"
if $dry; then
    echo "Would set version = \"$next\", run the tests and clippy, commit \"Release $tag\" and tag $tag$($push && echo ', then push both to origin')."
    exit 0
fi

# Only the [workspace.package] version; dependency versions sit inside braces or under other tables.
awk -v next_version="$next" '
    /^\[/ { in_package = ($0 == "[workspace.package]") }
    in_package && /^version = / { $0 = "version = \"" next_version "\""; done = 1 }
    { print }
    END { if (!done) exit 1 }
' Cargo.toml > Cargo.toml.new || { rm -f Cargo.toml.new; fail "no version under [workspace.package] in Cargo.toml"; }
mv Cargo.toml.new Cargo.toml
cargo update --workspace --offline --quiet

restore() { git checkout -- Cargo.toml Cargo.lock; }
if ! cargo test --workspace --locked --quiet || ! cargo clippy --workspace --all-targets --locked --quiet -- -D warnings; then
    restore
    fail "tests or clippy failed; nothing was committed or tagged"
fi

git commit --quiet -m "Release $tag" Cargo.toml Cargo.lock
git tag -a "$tag" -m "Darkroom $tag"
echo "Committed and tagged $tag."

if $push; then
    git push --atomic origin "$branch" "refs/tags/$tag"
    echo "Pushed. The release workflow builds macOS, Linux and Windows and publishes the $tag release."
else
    echo "To release it: git push --atomic origin $branch refs/tags/$tag"
fi
