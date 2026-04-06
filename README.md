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

## cour TUI

Launch the TUI with:

```bash
cargo run -p cour --bin cour
# or
cargo run -p cour --bin cour -- tui
```

If no config exists yet, `cour` launches the setup wizard automatically and helps create `~/.config/cour/config.toml`.
You can also rerun onboarding explicitly:

```bash
cargo run -p cour --bin cour -- setup
```

Daily-use basics:

- switch workspaces between brief, inbox, thread, search, actions, and drafts from the keyboard
- press `?` for the keymap
- press `q` to quit
- rerun onboarding with `cour setup`
