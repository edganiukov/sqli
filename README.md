# sqli

A terminal-based SQL client with vim-style keybindings. Supports multiple database backends.

## Supported Databases

- **PostgreSQL** - native protocol (port 5432).
- **Cassandra/ScyllaDB** - CQL protocol (port 9042).
- **ClickHouse** - HTTP API (port 8123).

## Features

- Multi-tab interface.
- Sidebar with databases and tables.
- Query editor with vim-style navigation.
- Results viewer with scrolling.
- TOML configuration file with password command support.

## Keybindings

### Global
| Key | Action |
|-----|--------|
| `:` | Enter command mode |
| `Tab` | Cycle focus: Sidebar -> Query -> Output |
| `Shift+Tab` | Cycle focus backwards |
| `F5` / `Ctrl+J` | Execute query |

### Sidebar
| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate |
| `Enter` | Expand database / Preview table |
| `d` | Describe table structure |
| `r` | Refresh databases |
| `l` or `Right` | Move to query panel |

### Output
| Key | Action |
|-----|--------|
| `j/k` | Scroll line by line |
| `PageUp/PageDown` | Scroll by 10 rows |
| `gg` | Scroll to top |
| `G` | Scroll to bottom |

### Commands
| Command | Action |
|---------|--------|
| `:q` | Close current tab (quit if last) |
| `:qa` | Quit application |
| `:new` | New tab |
| `:next` / `:prev` | Switch tabs |
| `:sysdb` | Toggle system databases |
| `:help` | Show help |

## Configuration

Configuration is loaded from:
1. `./config.toml` (current directory)
2. `~/.config/sqli/config.toml`

### Example config.toml

```toml
[local-postgres]
type = "postgres"
host = "localhost"
port = 5432
user = "postgres"
password = "secret"

[production]
type = "postgres"
host = "db.example.com"
user = "admin"
password_cmd = "pass show db/production"

[analytics]
type = "clickhouse"
host = "clickhouse.local"
port = 8123
user = "default"

[scylla-cluster]
type = "cassandra"
host = "scylla1.local"
port = 9042
user = "cassandra"
password = "cassandra"
```

## Building

Requires Rust 1.85+ (edition 2024).

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run directly
cargo run
```

Binary will be at `target/release/sqli`.

## License

MIT
