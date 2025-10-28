-- Initialization script for Alle testing database
-- This script runs automatically when the PostgreSQL container starts for the first time

-- Create a sample table for testing
CREATE TABLE IF NOT EXISTS orders (
    id SERIAL PRIMARY KEY,
    customer_name VARCHAR(255) NOT NULL,
    total DECIMAL(10, 2) NOT NULL,
    status VARCHAR(50) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create a sample table for notifications log
CREATE TABLE IF NOT EXISTS notification_log (
    id SERIAL PRIMARY KEY,
    channel VARCHAR(255) NOT NULL,
    payload TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Function to automatically send NOTIFY when a new order is created
CREATE OR REPLACE FUNCTION notify_new_order()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('orders', json_build_object(
        'id', NEW.id,
        'customer_name', NEW.customer_name,
        'total', NEW.total,
        'status', NEW.status
    )::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger for new orders
DROP TRIGGER IF EXISTS order_notify_trigger ON orders;
CREATE TRIGGER order_notify_trigger
    AFTER INSERT ON orders
    FOR EACH ROW
    EXECUTE FUNCTION notify_new_order();

-- Function to log all notifications
CREATE OR REPLACE FUNCTION log_notification()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO notification_log (channel, payload)
    VALUES (TG_ARGV[0], NEW.*::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Insert some sample data
INSERT INTO orders (customer_name, total, status) VALUES
    ('Alice Johnson', 99.99, 'pending'),
    ('Bob Smith', 149.50, 'completed'),
    ('Charlie Brown', 75.25, 'pending');

-- Simple auth function for testing
CREATE OR REPLACE FUNCTION authenticate_user(token TEXT)
RETURNS TABLE(user_id TEXT, authenticated BOOLEAN) AS $$
BEGIN
    IF token = 'valid_token' THEN
        RETURN QUERY SELECT 'user_123'::TEXT, TRUE;
    ELSE
        RETURN QUERY SELECT ''::TEXT, FALSE;
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Even simpler boolean-only auth function
CREATE OR REPLACE FUNCTION verify_token(token TEXT)
RETURNS BOOLEAN AS $$
BEGIN
    RETURN token = 'valid_token';
END;
$$ LANGUAGE plpgsql;

-- Print success message
DO $$
BEGIN
    RAISE NOTICE 'Alle test database initialized successfully!';
    RAISE NOTICE 'Sample orders table created with % rows', (SELECT COUNT(*) FROM orders);
    RAISE NOTICE 'Notification trigger installed on orders table';
    RAISE NOTICE 'Auth functions created: authenticate_user, verify_token';
END $$;
