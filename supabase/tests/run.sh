#!/usr/bin/env bash
# 部屋の権限の試験を、使い捨ての Postgres で流す。docker が要る。
set -euo pipefail
cd "$(dirname "$0")/.."
name="asis-rls-$$"
docker run -d --rm --name "$name" -e POSTGRES_PASSWORD=x postgres:16-alpine >/dev/null
trap 'docker stop "$name" >/dev/null' EXIT
until docker exec "$name" pg_isready -U postgres -q; do sleep 0.3; done; sleep 1
run() { docker exec -i "$name" psql -U postgres -q -v ON_ERROR_STOP=1 "$@"; }
run < tests/stub.sql
for f in migrations/*.sql; do run < "$f"; done
run < tests/rls.sql
