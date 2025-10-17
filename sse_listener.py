#!/usr/bin/env python3
"""
Simple SSE (Server-Sent Events) listener for the Alle bridge.

Usage:
    python sse_listener.py <channel> [--url URL]

Example:
    python sse_listener.py notifications
    python sse_listener.py test_channel --url http://localhost:8080
"""

import argparse
import sys
import signal
from datetime import datetime


def listen_to_sse(url: str, channel: str):
    """
    Connect to SSE endpoint and log all events.

    Args:
        url: Base URL of the server (e.g., http://localhost:8080)
        channel: Channel name to listen to
    """
    import requests

    endpoint = f"{url}/events?channels={channel}"
    print(f"Connecting to SSE endpoint: {endpoint}")
    print(f"Listening to channel: {channel}")
    print("Press Ctrl+C to stop\n")

    try:
        # Stream the SSE connection
        response = requests.get(endpoint, stream=True, timeout=None)
        response.raise_for_status()

        print(f"Connected! Waiting for events...\n")

        # Process the stream line by line
        for line in response.iter_lines(decode_unicode=True):
            if line:
                timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S.%f")[:-3]

                # SSE format: "event: <event_type>" or "data: <payload>"
                if line.startswith("data:"):
                    data = line[5:].strip()  # Remove "data:" prefix
                    if data != "keep-alive":  # Skip keep-alive messages
                        print(f"[{timestamp}] {data}")
                elif line.startswith("event:"):
                    event_type = line[6:].strip()
                    # Log event type if it's not the default "message"
                    if event_type != "message":
                        print(f"[{timestamp}] Event type: {event_type}")

    except KeyboardInterrupt:
        print("\n\nDisconnected by user")
        sys.exit(0)
    except requests.exceptions.RequestException as e:
        print(f"\nConnection error: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"\nUnexpected error: {e}", file=sys.stderr)
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(
        description="Listen to Server-Sent Events from Alle bridge",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s notifications
  %(prog)s test_channel --url http://localhost:8080
        """
    )
    parser.add_argument(
        "channel",
        help="Channel name to listen to"
    )
    parser.add_argument(
        "--url",
        default="http://127.0.0.1:8080",
        help="Base URL of the SSE server (default: http://127.0.0.1:8080)"
    )

    args = parser.parse_args()

    # Handle Ctrl+C gracefully
    signal.signal(signal.SIGINT, lambda s, f: sys.exit(0))

    listen_to_sse(args.url, args.channel)


if __name__ == "__main__":
    main()
