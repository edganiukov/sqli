-- ClickHouse initialization script
CREATE DATABASE IF NOT EXISTS testdb;

CREATE TABLE testdb.users (
    id UInt32,
    username String,
    email String,
    created_at DateTime DEFAULT now()
) ENGINE = MergeTree()
ORDER BY id;

CREATE TABLE testdb.orders (
    id UInt32,
    user_id UInt32,
    total Decimal(10, 2),
    status String DEFAULT 'pending',
    created_at DateTime DEFAULT now()
) ENGINE = MergeTree()
ORDER BY id;

CREATE TABLE testdb.products (
    id UInt32,
    name String,
    price Decimal(10, 2),
    stock UInt32 DEFAULT 0
) ENGINE = MergeTree()
ORDER BY id;

INSERT INTO testdb.users (id, username, email) VALUES
    (1, 'alice', 'alice@example.com'),
    (2, 'bob', 'bob@example.com'),
    (3, 'charlie', 'charlie@example.com');

INSERT INTO testdb.products (id, name, price, stock) VALUES
    (1, 'Widget', 9.99, 100),
    (2, 'Gadget', 24.99, 50),
    (3, 'Gizmo', 14.99, 75);

INSERT INTO testdb.orders (id, user_id, total, status) VALUES
    (1, 1, 34.98, 'completed'),
    (2, 2, 9.99, 'pending'),
    (3, 1, 24.99, 'shipped');
