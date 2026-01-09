# sqli

A terminal-based SQL client with vim-style keybindings. Supports multiple database backends.

**NOTE: the project is vibe-coded.**

## Supported Databases

- **PostgreSQL** - native protocol.
- **MySQL/MariaDB** - native protocol.
- **Cassandra/ScyllaDB** - CQL protocol.
- **ClickHouse** - HTTP API.

## Features

- Multi-tab interface.
- Sidebar with databases and tables.
- Query editor with vim-style navigation.
- Results viewer with scrolling.
- TOML configuration file with password command support.
- Query templates with `<placeholder>` support.
- External editor integration (`$EDITOR` or vim).

## Screenshots

![Database View](examples/screenshots/db_view.png)

## Keybindings

### Global
| Key | Action |
|-----|--------|
| `:` | Enter command mode |
| `Tab` | Cycle focus: Sidebar -> Query -> Output |
| `Shift+Tab` | Cycle focus backwards |
| `F5` / `Ctrl+J` | Execute query |

### Query Editor
| Key | Action |
|-----|--------|
| `Ctrl+O` | Open templates list |
| `Ctrl+S` | Save current query as template |
| `Ctrl+G` | Edit query in external editor |

### Templates Popup
| Key | Action |
|-----|--------|
| `j/k` or arrows | Navigate templates |
| `Enter` | Insert selected template |
| `Ctrl+G` | Edit template in external editor |
| `Ctrl+D` | Delete template |
| `Esc` | Close popup |

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

Configuration is loaded from (in order of priority):
1. `--config` flag (e.g., `sqli --config /path/to/config.toml`)
2. `./sqli.toml` (current directory)
3. `~/.config/sqli/config.toml`

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

[local-mysql]
type = "mysql"
host = "localhost"
port = 3306
user = "root"
password = "secret"

[analytics]
type = "clickhouse"
host = "clickhouse.local"
port = 8123
user = "default"

[analytics-secure]
type = "clickhouse"
host = "clickhouse.example.com"
port = 8443
user = "default"
tls = true

[scylla-cluster]
type = "cassandra"
host = "scylla1.local"
port = 9042
user = "cassandra"
password = "cassandra"
```

## Query Templates

Templates are saved to `~/.config/sqli/templates.sql` in a simple format:

```sql
=== Count Rows [global]
SELECT COUNT(*) FROM <table>

=== Active Users [my-connection]
SELECT * FROM users
WHERE active = true
LIMIT <limit>

=== Find by ID [global]
SELECT * FROM <table> WHERE id = <id>
```

- Each template starts with `=== Name [scope]`
- Scope is either `global` (all connections) or a connection name
- Templates can contain `<placeholder>` markers - cursor will be positioned there on insert
- Use `Ctrl+S` to save the current query as a template
- Use `Ctrl+O` to open the templates list
- Use `Ctrl+G` to edit a template in your `$EDITOR`

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
