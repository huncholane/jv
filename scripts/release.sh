#!/bin/bash
# Author: Lane Henslee (huncho@hygo.ai)
# Bumps the version for relevant version files, auto commits them, and prompts a tag with editor.

current_version="$(git tag -l | head -n 1 | sed 's/^v//')"
[[ -z "$current_version" ]] && current_version=0.0.0
echo -e "\033[35mCurrently on version $current_version\033[0m"

IFS='.' read -r major minor patch <<<"$current_version"

case "$1" in
same) echo -e "\033[34mKeeping the same version\033[0m" ;;
major) major=$((major + 1)) && minor=0 && patch=0 ;;
minor) minor=$((minor + 1)) && patch=$((patch + 1)) ;;
*) patch=$((patch + 1)) ;;
esac

new_version="$major.$minor.$patch"

echo -e "\033[32mUpdating version $new_version\033[0m"

printf "\033[37m(y/n) Would you like to continue? Defaults to y: \033[0m"
read -rp "" yn
{ [[ "$yn" == "y" ]] || [[ -z "$yn" ]]; } || { echo -e "\033[31mAborting...\033[0m" && exit 0; }

find . -name Cargo.toml -exec sed -i "s/^version.*/version = \"$new_version\"/" {} \;
find . -name Cargo.toml -exec git add {} \;
find . -name package.json -exec sed -i "s/\"version\".*/\"version\": \"$new_version\"/" {} \;
find . -name package.json -exec git add {} \;

echo -e "\033[37mUpdated relevant files. Create new commit\033[0m"
git commit -m "version tag"

echo -e "\033[37mCompleted commit. Create new tag\033[0m"
git tag -f -a "v$new_version"
