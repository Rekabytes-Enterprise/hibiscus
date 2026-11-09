#!/usr/bin/env python3
"""One JSON request in, one JSON reply out. No Hibiscus SDK required."""
import json
import sys


def main():
    request = json.load(sys.stdin)
    if request["apiVersion"] != 1 or request["name"] != "greet":
        raise ValueError("Unsupported request")
    if request["kind"] == "tool":
        name = request["arguments"]["name"]
    elif request["kind"] == "command":
        name = request["arguments"]["input"].strip() or "friend"
    else:
        raise ValueError("Unsupported invocation kind")
    print(json.dumps({
        "apiVersion": 1,
        "id": request["id"],
        "text": f"Hello, {name}!",
        "isError": False,
        "data": {"workspace": request["cwd"]},
    }))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Greeting extension failed. Check the request and extension code.", file=sys.stderr)
        sys.exit(1)
