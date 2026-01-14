-- PostgreSQL initialization script
CREATE DATABASE testdb;

\c testdb

CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(50) NOT NULL,
    email VARCHAR(100) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    total DECIMAL(10, 2) NOT NULL,
    status VARCHAR(20) DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE products (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    price DECIMAL(10, 2) NOT NULL,
    stock INTEGER DEFAULT 0
);

INSERT INTO users (username, email) VALUES
    ('alice', 'alice@example.com'),
    ('bob', 'bob@example.com'),
    ('charlie', 'charlie@example.com');

INSERT INTO products (name, price, stock) VALUES
    ('Widget', 9.99, 100),
    ('Gadget', 24.99, 50),
    ('Gizmo', 14.99, 75);

INSERT INTO orders (user_id, total, status) VALUES
    (1, 34.98, 'completed'),
    (2, 9.99, 'pending'),
    (1, 24.99, 'shipped');

CREATE TABLE events (
    id SERIAL PRIMARY KEY,
    event_type VARCHAR(50) NOT NULL,
    payload TEXT,
    metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO events (event_type, payload, metadata) VALUES
    ('user.signup', '{"user_id": 1, "username": "alice", "email": "alice@example.com", "preferences": {"theme": "dark", "notifications": {"email": true, "push": false, "sms": false}, "language": "en-US", "timezone": "America/New_York"}, "device": {"type": "mobile", "os": "iOS", "version": "17.0", "browser": "Safari"}}', '{"ip": "192.168.1.100", "user_agent": "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1", "referrer": "https://google.com/search?q=best+widgets", "session_id": "sess_abc123xyz789"}'),
    ('order.created', '{"order_id": 1, "user_id": 1, "items": [{"product_id": 1, "name": "Widget", "quantity": 2, "price": 9.99, "discount": 0}, {"product_id": 2, "name": "Gadget", "quantity": 1, "price": 24.99, "discount": 5.00}], "shipping": {"method": "express", "address": {"street": "123 Main Street, Apartment 4B", "city": "New York", "state": "NY", "zip": "10001", "country": "USA"}, "estimated_delivery": "2024-01-15"}, "payment": {"method": "credit_card", "last4": "4242", "brand": "visa"}}', '{"ip": "192.168.1.100", "correlation_id": "corr_order_001_abc", "feature_flags": ["new_checkout", "express_shipping"]}'),
    ('error.occurred', 'Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. Curabitur pretium tincidunt lacus. Nulla gravida orci a odio. Nullam varius, turpis et commodo pharetra.', '{"error_code": "ERR_500", "stack_trace": "at Object.handleRequest (/app/src/handlers/order.js:156:23)\n    at Router.dispatch (/app/node_modules/express/lib/router/index.js:284:7)\n    at Layer.handle (/app/node_modules/express/lib/router/layer.js:95:5)\n    at next (/app/node_modules/express/lib/router/index.js:166:14)"}')
