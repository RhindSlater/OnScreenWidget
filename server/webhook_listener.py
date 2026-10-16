#!/usr/bin/env python3
"""
OnScreen Widget — webhook command listener.

A tiny, dependency-free HTTP listener that runs a FIXED allow-list of commands
when pinged with the correct auth token from a whitelisted IP. Intended to let a
widget tile click trigger an action (e.g. restart a service) without SSH.

SECURITY MODEL (all three layers apply to every request):
  1. IP allow-list  — only whitelisted client IPs are served; others get 403.
  2. Auth token     — a secret token must match the X-Auth-Token header; else 403.
  3. Command allow-list — the request only names a KEY; the actual command is
                          hardcoded here. Request content is NEVER executed as a
                          shell command, so there is no command-injection surface.

Requests:  POST /run/<key>   with header  X-Auth-Token: <token>
Response:  JSON { ok, key, exit_code, stdout, stderr } (truncated).

Configure via environment variables (see the systemd unit) or edit DEFAULTS.
"""

import json
import logging
import os
import shlex
import subprocess
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# --- Configuration ----------------------------------------------------------
# Prefer environment variables (set by the systemd unit); fall back to these.

# Address/port to bind. Use 0.0.0.0 to listen on all interfaces, or a specific
# LAN IP. Keep this OFF the public internet unless you add TLS + firewalling.
HOST = os.environ.get("WEBHOOK_HOST", "0.0.0.0")
PORT = int(os.environ.get("WEBHOOK_PORT", "8787"))

# Shared secret. Generate with:  python3 -c "import secrets;print(secrets.token_urlsafe(32))"
# Set WEBHOOK_TOKEN in the environment; the empty default refuses all requests.
AUTH_TOKEN = os.environ.get("WEBHOOK_TOKEN", "")

# Comma-separated list of client IPs allowed to call this listener.
# e.g. WEBHOOK_ALLOWED_IPS="192.168.1.20,192.168.1.21"
ALLOWED_IPS = {
    ip.strip()
    for ip in os.environ.get("WEBHOOK_ALLOWED_IPS", "").split(",")
    if ip.strip()
}

# The command allow-list: maps a request key -> an exact command (as an argv
# list, NOT a shell string). Edit this to the commands you want to allow.
# Each value is passed to subprocess WITHOUT a shell, so no injection is possible.
COMMANDS = {
    "restart-minecraft": ["sudo", "systemctl", "restart", "minecraft"],
    "restart-jellyfin": ["sudo", "systemctl", "restart", "jellyfin"],
    # Add your own, e.g.:
    # "reboot": ["sudo", "systemctl", "reboot"],
}

# How long a command may run before being killed (seconds).
COMMAND_TIMEOUT = int(os.environ.get("WEBHOOK_TIMEOUT", "30"))

# Max bytes of stdout/stderr returned in the JSON response.
MAX_OUTPUT = 2000

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger("webhook")


def _client_ip(handler: BaseHTTPRequestHandler) -> str:
    # Direct peer address. (If behind a trusted reverse proxy you would read
    # X-Forwarded-For instead — not done here to avoid spoofable headers.)
    return handler.client_address[0]


def _constant_time_eq(a: str, b: str) -> bool:
    # Avoid timing side-channels on the token comparison.
    import hmac

    return hmac.compare_digest(a.encode(), b.encode())


class Handler(BaseHTTPRequestHandler):
    server_version = "OSWWebhook/1.0"

    def _send(self, code: int, payload: dict) -> None:
        body = json.dumps(payload).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args) -> None:  # route through logging
        log.info("%s - %s", self.address_string(), fmt % args)

    def do_GET(self) -> None:
        # Health check only; never runs commands.
        if self.path == "/health":
            self._send(200, {"ok": True, "service": "osw-webhook"})
        else:
            self._send(404, {"ok": False, "error": "not found"})

    def do_POST(self) -> None:
        ip = _client_ip(self)

        # Layer 1: IP allow-list.
        if ALLOWED_IPS and ip not in ALLOWED_IPS:
            log.warning("denied: ip %s not in allow-list", ip)
            self._send(403, {"ok": False, "error": "forbidden"})
            return

        # Layer 2: auth token.
        token = self.headers.get("X-Auth-Token", "")
        if not AUTH_TOKEN or not _constant_time_eq(token, AUTH_TOKEN):
            log.warning("denied: bad/missing token from %s", ip)
            self._send(403, {"ok": False, "error": "forbidden"})
            return

        # Route: /run/<key>
        if not self.path.startswith("/run/"):
            self._send(404, {"ok": False, "error": "not found"})
            return
        key = self.path[len("/run/"):].strip("/")

        # Layer 3: command allow-list (exact, hardcoded argv).
        cmd = COMMANDS.get(key)
        if cmd is None:
            log.warning("denied: unknown command key '%s' from %s", key, ip)
            self._send(404, {"ok": False, "error": "unknown command"})
            return

        log.info("running '%s' (%s) for %s", key, shlex.join(cmd), ip)
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=COMMAND_TIMEOUT,
                check=False,
            )
            self._send(
                200 if proc.returncode == 0 else 500,
                {
                    "ok": proc.returncode == 0,
                    "key": key,
                    "exit_code": proc.returncode,
                    "stdout": proc.stdout[:MAX_OUTPUT],
                    "stderr": proc.stderr[:MAX_OUTPUT],
                },
            )
        except subprocess.TimeoutExpired:
            log.error("command '%s' timed out", key)
            self._send(504, {"ok": False, "key": key, "error": "timeout"})
        except Exception as e:  # noqa: BLE001 - report any run failure
            log.exception("command '%s' failed", key)
            self._send(500, {"ok": False, "key": key, "error": str(e)})


def main() -> int:
    if not AUTH_TOKEN:
        log.error(
            "WEBHOOK_TOKEN is not set — refusing to start. "
            'Generate one: python3 -c "import secrets;print(secrets.token_urlsafe(32))"'
        )
        return 1
    if not ALLOWED_IPS:
        log.warning(
            "WEBHOOK_ALLOWED_IPS is empty — all IPs will be allowed (token still required). "
            "Set it to restrict to your widget machine(s)."
        )
    srv = ThreadingHTTPServer((HOST, PORT), Handler)
    log.info("listening on %s:%s | commands: %s", HOST, PORT, ", ".join(COMMANDS))
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
