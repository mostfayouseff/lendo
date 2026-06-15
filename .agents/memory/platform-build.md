---
name: Platform build lessons
description: Key decisions and fixes made during the Apex Flash Loan Platform build to avoid repeating them.
---

# SQLx compile-time macros require a live database

SQLx `query_as!` and `query!` macros connect to `DATABASE_URL` at compile time.
If tables don't exist, compilation fails with "relation X does not exist".
**Fix**: run all SQL migrations before `cargo check` (psql `$DATABASE_URL -f migration.sql`).

**Why**: sqlx compile-time checking is a hard requirement, not optional.

**How to apply**: always run `psql $DATABASE_URL -f database/migrations/*.sql` before first build.

# INET PostgreSQL type requires the `ipnetwork` crate

If a column type is INET, sqlx requires the `ipnetwork` feature and the `ipnetwork` Rust crate.
**Fix**: use migration to ALTER COLUMN ip_address TYPE TEXT — simpler and avoids extra dependency.

# SQLx enums in query macros require Copy or clone

`req.field as EnumType` in sqlx macros tries to move the value.
If the struct is borrowed (`&CreateFoo`), this fails unless the enum derives Copy.
**Fix**: add `Copy` to all fieldless enum derives. Since all our enums are fieldless, this is safe.

# error.rs IntoResponse must match self, not &self

`match &self` in `IntoResponse` causes borrow errors when calling `e.into_response()` (takes ownership).
**Fix**: `match self` (consume the enum) and handle each variant independently.

# COALESCE(SUM(bigint), 0) returns Decimal in sqlx

SQLx infers `SUM(bigint)` as `Decimal` (PostgreSQL NUMERIC). Adding `::bigint` cast in SQL fixes it.
**Fix**: `COALESCE(SUM(col), 0)::bigint AS alias` → sqlx sees `i64`.

# Decimal::from_f64_retain for f64 → Decimal conversion

`Decimal::try_from(f64)` is not in the public API of rust_decimal 1.x.
**Fix**: use `Decimal::from_f64_retain(v)` which returns `Option<Decimal>`.
