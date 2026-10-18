#!/usr/bin/env bash
# Generates api/migrations from the feature schemas, then makes every created table STRICT.
set -euo pipefail

cd "$(dirname "$0")/../api"
if ! compgen -G "src/features/*/model.ts" >/dev/null; then
    echo "no src/features/*/model.ts yet; nothing to generate"
    exit 0
fi
bun run db:generate "$@"
# Drizzle cannot emit STRICT. `[^;]*` stops at the statement's end, so a table that already ends
# in `) STRICT;` is left alone and applied files keep their hash. Rebuild tables (`__new_*`)
# are CREATE TABLE statements too.
for file in migrations/*.sql; do
    perl -0pi -e 's/(CREATE TABLE [^;]*\n\));/$1 STRICT;/g' "$file"
done
