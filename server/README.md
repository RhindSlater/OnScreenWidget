# OnScreen Widget — Webhook Command Listener

A tiny, dependency-free HTTP listener that runs a **fixed allow-list** of
commands when pinged with the correct **auth token** from a **whitelisted IP**.
This lets a widget tile click trigger an action (e.g. restart a service)
**without SSH**.

## Security model (three layers, all enforced on every request)

1. **IP allow-list** — only whitelisted client IPs are served; others get 403.
2. **Auth token** — a secret token must match the `X-Auth-Token` header.
3. **Command allow-list** — the request only names a *key*; the actual command
   is hardcoded in the script as an argv list and run **without a shell**, so
   there is no command-injection surface. The request body/content is never
   executed.

> Keep this on your **LAN/VPN**. Do not expose it to the public internet without
> adding TLS and tight firewalling. Even then, prefer a VPN.

## Files

- `webhook_listener.py` — the listener (Python 3.8+ stdlib only).
- `osw-webhook.service` — a systemd unit to run it on boot.

## Setup (Linux / systemd)

1. **Pick the commands you want to allow.** Edit the `COMMANDS` map near the top
   of `webhook_listener.py`. Each entry maps a URL key to an exact argv list:

   ```python
   COMMANDS = {
       "restart-minecraft": ["sudo", "systemctl", "restart", "minecraft"],
       "restart-jellyfin":  ["sudo", "systemctl", "restart", "jellyfin"],
   }
   ```

2. **Install the script:**
   ```bash
   sudo mkdir -p /opt/osw-webhook
   sudo cp webhook_listener.py /opt/osw-webhook/
   ```

3. **Create a dedicated low-privilege user** to run it (not root):
   ```bash
   sudo useradd --system --no-create-home --shell /usr/sbin/nologin webhook
   ```

4. **Generate an auth token:**
   ```bash
   python3 -c "import secrets;print(secrets.token_urlsafe(32))"
   ```

5. **Install and edit the service unit:**
   ```bash
   sudo cp osw-webhook.service /etc/systemd/system/
   sudo nano /etc/systemd/system/osw-webhook.service
   ```
   Set:
   - `WEBHOOK_TOKEN=` the token from step 4
   - `WEBHOOK_ALLOWED_IPS=` your widget machine's IP (comma-separated for several)
   - `WEBHOOK_PORT=` a port of your choice (default 8787)

6. **Allow the service user to run just those commands via sudo** (needed because
   `systemctl restart` requires privilege). Create a tightly-scoped sudoers file —
   list the EXACT commands, nothing broader:
   ```bash
   sudo visudo -f /etc/sudoers.d/osw-webhook
   ```
   ```
   webhook ALL=(root) NOPASSWD: /usr/bin/systemctl restart minecraft, /usr/bin/systemctl restart jellyfin
   ```
   (Match the paths/commands to your `COMMANDS` map. Keep this list minimal.)

7. **Start it:**
   ```bash
   sudo systemctl daemon-reload
   sudo systemctl enable --now osw-webhook
   sudo systemctl status osw-webhook
   ```

8. **Test from the widget machine** (replace IP/port/token):
   ```bash
   curl -s -X POST -H "X-Auth-Token: YOUR_TOKEN" http://SERVER_IP:8787/run/restart-minecraft
   # Health check (no token needed):
   curl -s http://SERVER_IP:8787/health
   ```

## Firewall (recommended)

Restrict the port to your LAN / widget host at the OS firewall too, as defense
in depth beyond the app's IP allow-list. Example (ufw):
```bash
sudo ufw allow from 192.168.1.20 to any port 8787 proto tcp
```

## Using it from the widget

Add an **`http_request`** click action to a tile (Settings → edit tile → pick a
click → "HTTP request"):
- **Method:** POST
- **URL:** `http://SERVER_IP:8787/run/restart-minecraft`
- **Auth header name:** `X-Auth-Token`
- **Auth token secret:** store your token in Settings → Secrets (e.g. key
  `webhook_token`) and reference it here, so the token is kept in the OS
  keychain, not the plain config.

The widget fires the request in the background (no browser popup) and the tile
reflects success/failure.

## Logs

```bash
journalctl -u osw-webhook -f
```
Every allowed and denied request is logged with the client IP.
