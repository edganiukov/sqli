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

CREATE TABLE testdb.events (
    id UInt32,
    event_type String,
    payload String,
    metadata String,
    created_at DateTime DEFAULT now()
) ENGINE = MergeTree()
ORDER BY id;

INSERT INTO testdb.events (id, event_type, payload, metadata) VALUES
    (1, 'user.signup', '{"user_id": 1, "username": "alice", "email": "alice@example.com", "preferences": {"theme": "dark", "notifications": {"email": true, "push": false, "sms": false}, "language": "en-US", "timezone": "America/New_York"}, "device": {"type": "mobile", "os": "iOS", "version": "17.0", "browser": "Safari"}}', '{"ip": "192.168.1.100", "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15", "referrer": "https://google.com/search?q=best+widgets", "session_id": "sess_abc123xyz789"}'),
    (2, 'order.created', '{"order_id": 1, "user_id": 1, "items": [{"product_id": 1, "name": "Widget", "quantity": 2, "price": 9.99, "discount": 0}, {"product_id": 2, "name": "Gadget", "quantity": 1, "price": 24.99, "discount": 5.00}], "shipping": {"method": "express", "address": {"street": "123 Main Street, Apartment 4B", "city": "New York", "state": "NY", "zip": "10001", "country": "USA"}, "estimated_delivery": "2024-01-15"}, "payment": {"method": "credit_card", "last4": "4242", "brand": "visa"}}', '{"ip": "192.168.1.100", "correlation_id": "corr_order_001_abc", "feature_flags": ["new_checkout", "express_shipping"]}'),
    (3, 'error.occurred', 'Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.', '{"error_code": "ERR_500", "stack_trace": "at handleRequest (order.js:156)"}');
