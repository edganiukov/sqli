# sqli

A terminal-based SQL client. Supports multiple database backends.

**NOTE: the project is fully vibe-coded.**

## Supported Databases

- **PostgreSQL** - native protocol.
- **MySQL/MariaDB** - native protocol.
- **Cassandra/ScyllaDB** - CQL protocol.
- **ClickHouse** - HTTP API.

## Features

- Multi-tab interface.
- Sidebar with databases and tables.
- Query editor with external editor support.
- SQL autocompletion (keywords, tables, columns).
- Record detail popup for viewing full row data.
- Mouse support for navigation.
- TOML configuration file with password command support.
- Query templates with placeholders.
- External editor integration (`$EDITOR` or vim).

## Screenshots

![Database View](examples/screenshots/db_view.png)

## Keybindings

**Navigation**
- `Tab` / `Shift+Tab` - cycle focus
- `Ctrl+h/j/k/l` - directional focus

**Query Editor**
- `F5` / `Ctrl+R` - execute query
- `Ctrl+Space` - autocompletion (keywords, tables, columns)
- `Ctrl+O` - open templates
- `Ctrl+S` - save template
- `Ctrl+G` - external editor
- `Esc :` - command mode

**Sidebar**
- `j/k` - navigate
- `Enter` - expand/preview
- `d` - describe a table
- `r` - refresh the Sidebar

**Output**
- `j/k` - scroll rows
- `h/l` - scroll columns
- `^/$` - first/last column
- `gg/G` - go top/bottom
- `PageUp/Down` - page scroll
- `Enter` - open record detail popup

**Record Detail Popup**
- `j/k` - navigate fields
- `Esc` - close popup

**Mouse**
- Click on pane to focus
- Click on table in sidebar to select
- Click on row in output to open record detail
- Click on connection/database to connect

**Commands**
- `:q` - quit a tab
- `:qa` or `q!` - quit all tabs
- `:new` - new tab
- `:next/:prev` - switch tabs
- `:sysdb` - toggle system DBs

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
--- Count Rows [global]
SELECT COUNT(*) FROM <table>;

--- Active Users [my-connection]
SELECT * FROM users
WHERE active = true
LIMIT <limit>;

--- Find by ID [global]
SELECT * FROM <table> WHERE id = <id>;
```

- Each template starts with `--- Name [scope]`.
- Scope is either `global` (all connections) or a connection name.
- Templates can contain `<placeholder>` markers - cursor will be positioned there on insert.
- Use `Ctrl+S` to save the current query as a template.
- Use `Ctrl+O` to open the templates list.
- Use `/` to search templates by name (vim-style).
- Use `Ctrl+G` to edit a template in your `$EDITOR` (edits name, scope, and query).

## Autocompletion

Press `Ctrl+Space` in the query editor to trigger autocompletion:

- **Keywords** - SQL keywords (SELECT, FROM, WHERE, etc.)
- **Tables** - tables from current database (context-aware after FROM/JOIN)
- **Columns** - columns after typing `table.` or `alias.`

The completion popup shows:
- `k` - keyword
- `t` - table
- `c` - column

Navigate with `j/k` or arrows, apply with `Enter` or `Tab`, cancel with `Esc`.

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
