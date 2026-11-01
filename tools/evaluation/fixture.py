"""Deterministic synthetic Responses, an execution check rather than an AI score."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading


class Fixture:
    def __init__(self, batches, profile="repair"):
        self.batches = batches
        self.profile = profile
        self.requests = []
        self.errors = []
        fixture = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                self.connection.settimeout(5)
                try:
                    size = int(self.headers.get("Content-Length", "0"))
                    if not 0 < size <= 4 * 1024 * 1024 or self.path != "/v1/responses":
                        raise ValueError("invalid fixture request")
                    request = json.loads(self.rfile.read(size))
                    index = len(fixture.requests)
                    fixture.requests.append(request)
                    if fixture.profile == "provider_error":
                        self.send_error(503)
                        return
                    if fixture.profile == "claim_only" or index >= len(fixture.batches):
                        items = [{"type": "message", "role": "assistant",
                                  "content": [{"type": "output_text", "text": "Done. Tests passed."}]}]
                    else:
                        calls = json.loads(fixture.batches[index])
                        if fixture.profile == "bad_tool":
                            calls = [{"name": "unknown_evaluation_tool", "arguments": {}}]
                        items = [{"type": "function_call", "call_id": f"eval-{index}-{i}",
                                  "name": c["name"], "arguments": json.dumps(c["arguments"])}
                                 for i, c in enumerate(calls)]
                    response = {"status": "completed", "output": items}
                    if fixture.profile != "missing_usage":
                        response["usage"] = {"input_tokens": 20, "output_tokens": 5,
                                             "input_tokens_details": {"cached_tokens": 0}}
                    body = ("data: " + json.dumps({"type": "response.completed",
                                                   "response": response}) + "\n\n").encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.send_header("Content-Length", str(len(body)))
                    self.send_header("Connection", "close")
                    self.end_headers()
                    self.wfile.write(body)
                except (ValueError, OSError, KeyError, TypeError):
                    fixture.errors.append("fixture_protocol_error")
                    self.close_connection = True

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever,
                                       kwargs={"poll_interval": 0.02}, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    @property
    def base_url(self):
        return f"http://127.0.0.1:{self.server.server_port}/v1"

    def __exit__(self, *args):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
