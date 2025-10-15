#!/bin/bash
# Examples of running Alle with different CLI configurations

echo "=== Alle CLI Examples ==="
echo

echo "1. Show help:"
echo "   ./target/release/alle --help"
echo

echo "2. Show version:"
echo "   ./target/release/alle --version"
echo

echo "3. Run with defaults:"
echo "   ./target/release/alle"
echo "   # Connects to: postgresql://localhost/postgres"
echo "   # Listens on: 127.0.0.1:8080"
echo "   # No initial channels"
echo

echo "4. Run with custom Postgres URL:"
echo "   ./target/release/alle -p 'postgresql://user:pass@localhost/mydb'"
echo

echo "5. Run with custom WebSocket address:"
echo "   ./target/release/alle -w '0.0.0.0:9000'"
echo

echo "6. Run with initial channels:"
echo "   ./target/release/alle -c 'orders,alerts,notifications'"
echo

echo "7. Run with all options:"
echo "   ./target/release/alle \\"
echo "     -p 'postgresql://user:pass@localhost/mydb' \\"
echo "     -w '0.0.0.0:8080' \\"
echo "     -c 'orders,alerts' \\"
echo "     -l 'debug'"
echo

echo "8. Use environment variables:"
echo "   POSTGRES_URL='postgresql://localhost/mydb' \\"
echo "   WS_BIND_ADDR='0.0.0.0:8080' \\"
echo "   LISTEN_CHANNELS='channel1,channel2' \\"
echo "   ./target/release/alle"
echo

echo "9. Mix CLI args and env vars (CLI takes precedence):"
echo "   POSTGRES_URL='postgresql://localhost/postgres' \\"
echo "   ./target/release/alle -w '0.0.0.0:9000' -c 'events'"
echo

echo "10. Enable debug logging:"
echo "    ./target/release/alle -l 'alle=debug,debug'"
echo

echo "11. Production deployment example:"
echo "    ./target/release/alle \\"
echo "      -p 'postgresql://prod_user:secure_password@db.example.com/production' \\"
echo "      -w '0.0.0.0:8080' \\"
echo "      -l 'alle=info,warn'"
echo
