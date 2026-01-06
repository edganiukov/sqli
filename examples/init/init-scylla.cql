CREATE KEYSPACE IF NOT EXISTS testdb
    WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};

USE testdb;

CREATE TABLE users (
    id UUID PRIMARY KEY,
    username TEXT,
    email TEXT,
    created_at TIMESTAMP
);

CREATE TABLE orders (
    id UUID,
    user_id UUID,
    total DECIMAL,
    status TEXT,
    created_at TIMESTAMP,
    PRIMARY KEY (user_id, id)
);

CREATE TABLE products (
    id UUID PRIMARY KEY,
    name TEXT,
    price DECIMAL,
    stock INT
);

INSERT INTO users (id, username, email, created_at) VALUES
    (uuid(), 'alice', 'alice@example.com', toTimestamp(now()));
INSERT INTO users (id, username, email, created_at) VALUES
    (uuid(), 'bob', 'bob@example.com', toTimestamp(now()));
INSERT INTO users (id, username, email, created_at) VALUES
    (uuid(), 'charlie', 'charlie@example.com', toTimestamp(now()));

INSERT INTO products (id, name, price, stock) VALUES
    (uuid(), 'Widget', 9.99, 100);
INSERT INTO products (id, name, price, stock) VALUES
    (uuid(), 'Gadget', 24.99, 50);
INSERT INTO products (id, name, price, stock) VALUES
    (uuid(), 'Gizmo', 14.99, 75);
