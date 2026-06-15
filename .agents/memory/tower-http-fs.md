---
name: tower-http fs feature
description: ServeDir and ServeFile require the 'fs' cargo feature in tower-http.
---

`tower_http::services::ServeDir` and `ServeFile` are gated behind the `fs` feature flag. Without it, rustc reports "found an item that was configured out" — not a missing crate error.

**Fix in workspace Cargo.toml:**
```toml
tower-http = { version = "0.5", features = ["cors", "trace", "timeout", "fs"] }
```

**Why:** tower-http conditionally compiles file-serving to keep the crate lean for API-only users.
