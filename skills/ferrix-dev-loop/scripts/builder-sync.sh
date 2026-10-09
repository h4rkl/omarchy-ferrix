#!/bin/sh
# usage: builder-sync.sh -- push local HEAD to the builder and fast-forward its checkout.
# Commit first: the builder only sees committed work.
. "$(dirname -- "$0")/env.sh"
need FERRIX_REMOTE FERRIX_REMOTE_REPO
cd "$FERRIX_ROOT" || exit 1
git push -q "$FERRIX_REMOTE:$FERRIX_REMOTE_REPO" "HEAD:refs/heads/$FERRIX_REMOTE_BRANCH" || exit 1
remote "git merge -q --ff-only $FERRIX_REMOTE_BRANCH && git log --oneline -1"
