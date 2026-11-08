#!/bin/sh
set -eu
exec 9>/run/lock/poker-bank-deploy.lock
flock -n 9 || exit 0
ROOT=/opt/poker-bank
REPO=/var/lib/poker-bank-deploy/repo
export GIT_SSH_COMMAND='ssh -i /root/.ssh/poker-bank-deploy -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes'
if git -C "$REPO" ls-remote --exit-code origin refs/heads/deploy/linux-amd64 >/dev/null; then
  git -C "$REPO" fetch --quiet origin deploy/linux-amd64
else
  result=$?
  if [ "$result" -eq 2 ]; then echo "Waiting for first verified GitHub release"; exit 0; fi
  exit "$result"
fi
sha=$(git -C "$REPO" show FETCH_HEAD:COMMIT)
case "$sha" in ''|*[!a-f0-9]*) echo 'Invalid source revision' >&2; exit 1;; esac
[ "${#sha}" -eq 40 ] || exit 1
[ "$(cat "$ROOT/DEPLOYED_COMMIT" 2>/dev/null || true)" != "$sha" ] || exit 0
release="$ROOT/releases/$sha"
mkdir -p "$release"
git -C "$REPO" show FETCH_HEAD:release.tgz > "$release/release.tgz"
git -C "$REPO" show FETCH_HEAD:SHA256SUMS > "$release/SHA256SUMS"
(cd "$release" && sha256sum -c SHA256SUMS && tar -xzf release.tgz)
[ "$(cat "$release/COMMIT")" = "$sha" ] || exit 1
docker build --quiet -t "poker-bank:$sha" "$release"
previous=$(docker inspect --format '{{.Config.Image}}' poker-bank 2>/dev/null || true)
previous_sha=$(cat "$ROOT/DEPLOYED_COMMIT" 2>/dev/null || true)
# Preserve a consistent database snapshot before switching application versions.
if [ -n "$previous" ]; then
  if docker exec poker-bank test -x /app/poker-bank; then docker exec poker-bank /app/poker-bank backup
  else docker exec poker-bank node /app/backup.mjs
  fi
fi
start_release() {
  (cd "$ROOT" && POKER_IMAGE="$1" RELEASE_SHA="$2" docker compose up -d --no-build)
}
start_release "poker-bank:$sha" "$sha"
healthy=false
for attempt in $(seq 1 30); do
  if curl -fsS http://192.168.42.20:8087/healthz | grep -q "\"commit\":\"$sha\""; then healthy=true; break; fi
  sleep 2
done
if [ "$healthy" != true ]; then
  echo "Release $sha failed health verification; rolling back" >&2
  if [ -n "$previous" ]; then start_release "$previous" "$previous_sha"; fi
  exit 1
fi
printf '%s\n' "$sha" > "$ROOT/DEPLOYED_COMMIT"
echo "Deployed verified revision $sha"
