#!/bin/bash
# Test setup script for Alle
# This script starts PostgreSQL and provides examples for testing

set -e

echo "=== Alle Test Setup ==="
echo

# Check if docker-compose is available
if ! command -v docker-compose &>/dev/null && ! docker compose version &>/dev/null; then
  echo "Error: docker-compose is not installed"
  echo "Please install Docker Compose: https://docs.docker.com/compose/install/"
  exit 1
fi

# Determine which command to use
if command -v docker-compose &>/dev/null; then
  COMPOSE="docker-compose"
else
  COMPOSE="docker compose"
fi

echo "Starting PostgreSQL with Docker Compose..."
$COMPOSE up -d

echo "Waiting for PostgreSQL to be ready..."
until $COMPOSE exec -T postgres pg_isready -U postgres &>/dev/null; do
  echo -n "."
  sleep 1
done
echo

echo "✓ PostgreSQL is ready!"
echo

echo "Database details:"
echo "  Host: localhost"
echo "  Port: 5432"
echo "  User: postgres"
echo "  Password: postgres"
echo "  Database: postgres"
echo

echo "Connection string:"
echo "  postgresql://postgres:postgres@localhost/postgres"
echo

echo "Sample tables created:"
$COMPOSE exec -T postgres psql -U postgres -c "\dt" | grep -E "orders|notification_log" || echo "  (checking...)"
echo

echo "Sample orders:"
$COMPOSE exec -T postgres psql -U postgres -c "SELECT id, customer_name, total, status FROM orders;"
echo

echo "=== Next Steps ==="
echo

echo "1. Start the Alle bridge:"
echo "   cargo run"
echo "   # or with explicit config:"
echo "   cargo run -- -p 'postgresql://postgres:postgres@localhost/postgres'"
echo

echo "2. In another terminal, connect a WebSocket client:"
echo "   websocat ws://127.0.0.1:8080"
echo

echo "3. Subscribe to the 'orders' channel:"
echo "   {\"type\": \"subscribe\", \"channel\": \"orders\"}"
echo

echo "4. Test by inserting an order (in another terminal):"
echo "   $COMPOSE exec postgres psql -U postgres -c \\"
echo "     \"INSERT INTO orders (customer_name, total, status) VALUES ('Jane Doe', 199.99, 'pending');\""
echo

echo "5. Or send a manual notification:"
echo "   $COMPOSE exec postgres psql -U postgres -c \"NOTIFY orders, 'Manual test message';\""
echo

echo "6. When done testing, stop PostgreSQL:"
echo "   $COMPOSE down"
echo "   # Or to remove volumes:"
echo "   $COMPOSE down -v"
echo

echo "=== Quick Test Commands ==="
echo

echo "# Send a test notification:"
echo "$COMPOSE exec postgres psql -U postgres -c \"NOTIFY orders, 'Test message';\""
echo

echo "# Insert a new order (triggers automatic notification):"
echo "$COMPOSE exec postgres psql -U postgres -c \"INSERT INTO orders (customer_name, total, status) VALUES ('Test User', 50.00, 'pending');\""
echo

echo "# View all orders:"
echo "$COMPOSE exec postgres psql -U postgres -c \"SELECT * FROM orders;\""
echo

echo "# Connect to PostgreSQL shell:"
echo "$COMPOSE exec postgres psql -U postgres"
echo
