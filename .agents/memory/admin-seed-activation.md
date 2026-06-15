---
name: Admin seed activation
description: Admin user seeded at startup defaults to 'pending' status — must be explicitly activated.
---

The `users` table has `status user_status DEFAULT 'pending'`. The `create()` repository method does not set status, so any seeded admin account lands in `pending` state and login returns `ACCOUNT_PENDING`.

**Fix in seed_admin (state.rs):**
```rust
let user = self.users.create(&CreateUser { ... }, &hash).await?;
self.users.update(user.id, &UpdateUser {
    email: None, role: None,
    status: Some(UserStatus::Active),
}).await?;
```

**Why:** Keeping the DB default as 'pending' is correct for user-registered accounts (they need email verification or admin approval). Seeded admin accounts are operator-controlled and must be active immediately.

**How to apply:** Always call update() with Active status immediately after create() in any admin/operator seed function.
