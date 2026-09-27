#!/usr/bin/env python3
"""Serve web/ on localhost with caching disabled (so rebuilds show up on reload)."""
import functools, http.server, os, sys

port = int(sys.argv[1]) if len(sys.argv) > 1 else 8990
root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "web")


class NoCache(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


handler = functools.partial(NoCache, directory=root)
print(f"OG Paper spike: http://localhost:{port}")
http.server.ThreadingHTTPServer(("127.0.0.1", port), handler).serve_forever()
