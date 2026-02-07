# AGENTS.md - AI Coding Agent Guidelines

This document provides guidelines for AI coding agents working on the `sqli` codebase - a terminal-based SQL client supporting PostgreSQL, MySQL, Cassandra/ScyllaDB, ClickHouse, and SQLite.

## Build, Lint, and Test Commands

### Requirements
- Rust 1.85+ (edition 2024)
- Uses nightly features: let-chains (`if let ... && let ...`)

### Build
```bash
cargo build              # Debug build
cargo build --release    # Release build
```

### Run
```bash
cargo run                           # Run with default config
cargo run -- --config examples/sqli.toml  # Run with custom config
```

### Test
```bash
cargo test                    # Run all tests
cargo test <test_name>        # Run single test by name
cargo test <module>::         # Run all tests in a module
```

### Lint and Format
```bash
cargo fmt                 # Format code (uses default rustfmt)
cargo fmt -- --check      # Check formatting without changes
cargo clippy              # Run linter
cargo clippy --fix        # Auto-fix lint warnings
```

## Git
- Commit title in the following format - `<type>(<component>): <description>`
  The type is one of: `feat` (for feature), fix, chore (refactoring, cleanup).
  Add a body to a commit with short description (4-5 lines).

## Project Structure

```
src/
├── main.rs           # Entry point, terminal setup, event loop
├── app/              # UI rendering (ratatui widgets)
│   ├── mod.rs        # Main App struct and draw logic
│   ├── popups.rs     # Popup rendering (templates, record detail)
│   ├── theme.rs      # Color scheme definitions
│   └── widgets.rs    # Reusable widget components
├── config.rs         # TOML config loading
├── controller/       # Business logic
│   ├── mod.rs        # Core types (Mode, Focus, ViewState, Tab, Controller)
│   ├── input.rs      # Keyboard input handling
│   ├── navigation.rs # Focus/tab navigation, clipboard operations
│   ├── database.rs   # DB operations (connect, query, refresh)
│   ├── completion.rs # SQL autocompletion logic
│   └── templates.rs  # Template popup handling
├── db.rs             # DatabaseClient enum (dispatch layer)
├── error.rs          # Error types (SqliError, Result alias)
├── editor.rs         # External editor integration
├── templates.rs      # Template storage and parsing
├── completion.rs     # Completion types and suggestions
├── format.rs         # Output formatting utilities
├── debug.rs          # Debug logging utilities
├── postgres.rs       # PostgreSQL client
├── mysql.rs          # MySQL client
├── cassandra.rs      # Cassandra/ScyllaDB client
├── clickhouse.rs     # ClickHouse client (HTTP API)
└── sqlite.rs         # SQLite client
```

## Code Style Guidelines

### Import Organization
Order imports in this sequence, separated by blank lines:
1. Standard library (`std::`)
2. External crates
3. Internal modules (`crate::`)

### Formatting
- Uses default `rustfmt` (no custom config)
- 4-space indentation
- Keep lines under ~100 characters when practical
- Use trailing commas in multi-line constructs

### Naming Conventions
- `snake_case`: functions, variables, modules
- `PascalCase`: types, enums, traits
- `SCREAMING_SNAKE_CASE`: constants
- Prefix with `_` for intentionally unused: `_schema`
- Suffix `_mut` for mutable accessor methods: `current_tab_mut()`

### Async Patterns

- Use `tokio` runtime for all async database operations
- Bridge sync TUI with async DB calls using `runtime.block_on()`:
```rust
let result = self.runtime.block_on(async {
    let client = conn.create_client(&db_name).await?;
    client.execute_query(&query).await
});
```

### Let-Chains (Nightly Feature)
Use let-chains for cleaner conditional matching:
```rust
if let Some(ref db) = current
    && !tab.databases.contains(db)
{
    tab.current_database = tab.databases.first().cloned();
}
```

### Pattern Matching for Input Handling
```rust
match key_code {
    KeyCode::Char('j') | KeyCode::Down => self.move_down(),
    KeyCode::Char('k') | KeyCode::Up => self.move_up(),
    KeyCode::Enter => self.select(),
    _ => {}
}
```

## Key Design Decisions
1. **Controller pattern** - `Controller` owns all state; `App` handles rendering only
2. **Module-per-backend** - Each database has its own module with consistent interface
3. **TOML config** - Connections defined in `~/.config/sqli/config.toml`
