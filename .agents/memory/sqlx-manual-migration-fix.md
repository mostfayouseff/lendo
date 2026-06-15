---
name: SQLx manual migration fix
description: How to recover when DB migrations were applied via psql but _sqlx_migrations is empty.
---

When PostgreSQL migrations are applied manually (e.g. `psql $URL -f migration.sql`) instead of through SQLx's migrate!() macro, the `_sqlx_migrations` tracking table stays empty. On next startup, SQLx tries to re-run all migrations and fails with "type X already exists".

**Fix:** Compute SHA384 of each migration file and INSERT a record per migration:

```bash
for f in $(ls database/migrations/*.sql | sort); do
  version=$(echo $(basename "$f") | sed 's/^0*//' | sed 's/_.*//')
  desc=$(echo $(basename "$f") | sed 's/^[0-9]*_//' | sed 's/\.sql//' | sed 's/_/ /g')
  checksum=$(openssl dgst -sha384 "$f" | awk '{print $NF}')
  echo "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES ($version, '$desc', true, decode('$checksum', 'hex'), 0) ON CONFLICT DO NOTHING;"
done | psql "$DATABASE_URL"
```

**Why:** SQLx uses SHA384 stored as bytea. The checksum must match the file content exactly — use `openssl dgst -sha384` not sha384sum (both work but openssl is more portable in NixOS).

**How to apply:** Run after any manual psql migration run. If checksums were inserted with empty values (xxd not found), DELETE them first and reinsert.
