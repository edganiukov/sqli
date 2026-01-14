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

CREATE TABLE events (
    id UUID PRIMARY KEY,
    event_type TEXT,
    payload TEXT,
    metadata TEXT,
    created_at TIMESTAMP
);

INSERT INTO events (id, event_type, payload, metadata, created_at) VALUES
    (uuid(), 'user.signup', '{"user_id": 1, "username": "alice", "email": "alice@example.com", "preferences": {"theme": "dark", "notifications": {"email": true, "push": false, "sms": false}, "language": "en-US", "timezone": "America/New_York"}, "device": {"type": "mobile", "os": "iOS", "version": "17.0", "browser": "Safari"}}', '{"ip": "192.168.1.100", "user_agent": "Mozilla/5.0", "session_id": "sess_abc123xyz789"}', toTimestamp(now()));
INSERT INTO events (id, event_type, payload, metadata, created_at) VALUES
    (uuid(), 'order.created', '{"order_id": 1, "user_id": 1, "items": [{"product_id": 1, "name": "Widget", "quantity": 2, "price": 9.99}, {"product_id": 2, "name": "Gadget", "quantity": 1, "price": 24.99}], "shipping": {"method": "express", "address": {"street": "123 Main Street", "city": "New York", "state": "NY", "zip": "10001"}}}', '{"correlation_id": "corr_order_001_abc"}', toTimestamp(now()));
INSERT INTO events (id, event_type, payload, metadata, created_at) VALUES
    (uuid(), 'error.occurred', 'Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit.', '{"error_code": "ERR_500"}', toTimestamp(now()));
