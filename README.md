# cour crate

This crate contains the local-first `cour` client.

It owns:

- config loading
- Maildir discovery
- sync + reindex flows
- SQLite indexing
- query helpers
- semantic enrichment plumbing
- draft approval/send workflow
- CLI command handlers

Run locally with:

```bash
cargo run -p cour --bin cour -- --help
cargo test -p cour
```
