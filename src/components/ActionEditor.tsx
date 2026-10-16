import type { ActionSpec } from "../types";
import { PathInput } from "./PathInput";

interface Props {
  label: string; // "Single click", etc.
  value: ActionSpec | null | undefined;
  onChange: (next: ActionSpec | null) => void;
}

type ActionType = ActionSpec["type"] | "none";

const ACTION_TYPES: { value: ActionType; label: string }[] = [
  { value: "none", label: "— none —" },
  { value: "open_url", label: "Open URL" },
  { value: "ssh_terminal", label: "SSH in terminal" },
  { value: "ssh_command", label: "Run SSH command" },
  { value: "spawn_terminal", label: "Open terminal (program)" },
  { value: "launch_app", label: "Launch app / program" },
  { value: "http_request", label: "HTTP request (webhook)" },
  { value: "frontend", label: "App action" },
];

const FRONTEND_NAMES = [
  { value: "price_history", label: "Show price history" },
  { value: "open_latest_video", label: "Open next new video / YouTube home" },
  { value: "clear_badge", label: "Clear notifications" },
];

/** Build a default action object when switching to a new type. */
function defaultFor(type: ActionType): ActionSpec | null {
  switch (type) {
    case "none":
      return null;
    case "open_url":
      return { type: "open_url", url: "" };
    case "ssh_terminal":
      return { type: "ssh_terminal", host: "", user: "", port: 22 };
    case "ssh_command":
      return { type: "ssh_command", host: "", user: "", port: 22, command: "" };
    case "spawn_terminal":
      return { type: "spawn_terminal", program: "", args: [] };
    case "launch_app":
      return { type: "launch_app", path: "", args: [] };
    case "http_request":
      return {
        type: "http_request",
        method: "POST",
        url: "",
        auth_header: "X-Auth-Token",
        auth_token_secret_ref: "",
        body: "",
      };
    case "frontend":
      return { type: "frontend", name: "price_history" };
  }
}

export function ActionEditor({ label, value, onChange }: Props) {
  const type: ActionType = value?.type ?? "none";

  return (
    <div className="action-editor">
      <div className="field">
        <label>{label}</label>
        <select
          value={type}
          onChange={(e) => onChange(defaultFor(e.target.value as ActionType))}
        >
          {ACTION_TYPES.map((t) => (
            <option key={t.value} value={t.value}>
              {t.label}
            </option>
          ))}
        </select>
      </div>

      {value?.type === "open_url" && (
        <div className="field">
          <label>URL</label>
          <input
            value={value.url}
            placeholder="https://example.com"
            onChange={(e) => onChange({ ...value, url: e.target.value })}
          />
        </div>
      )}

      {(value?.type === "ssh_terminal" || value?.type === "ssh_command") && (
        <>
          <div className="field-row">
            <div className="field">
              <label>Host</label>
              <input
                value={value.host}
                placeholder="host or IP"
                onChange={(e) => onChange({ ...value, host: e.target.value })}
              />
            </div>
            <div className="field field--sm">
              <label>User</label>
              <input
                value={value.user ?? ""}
                placeholder="optional"
                onChange={(e) => onChange({ ...value, user: e.target.value })}
              />
            </div>
            <div className="field field--xs">
              <label>Port</label>
              <input
                type="number"
                value={value.port}
                onChange={(e) =>
                  onChange({ ...value, port: Number(e.target.value) || 22 })
                }
              />
            </div>
          </div>
          {value.type === "ssh_command" && (
            <div className="field">
              <label>Command</label>
              <input
                value={value.command}
                placeholder="sudo systemctl restart myservice"
                onChange={(e) => onChange({ ...value, command: e.target.value })}
              />
            </div>
          )}
        </>
      )}

      {value?.type === "spawn_terminal" && (
        <div className="field-row">
          <div className="field">
            <label>Program</label>
            <input
              value={value.program}
              placeholder="e.g. claude, python, bash"
              onChange={(e) => onChange({ ...value, program: e.target.value })}
            />
          </div>
          <div className="field">
            <label>Args (space-separated)</label>
            <input
              value={value.args.join(" ")}
              placeholder="--chat"
              onChange={(e) =>
                onChange({
                  ...value,
                  args: e.target.value.split(/\s+/).filter(Boolean),
                })
              }
            />
          </div>
        </div>
      )}

      {value?.type === "launch_app" && (
        <>
          <div className="field">
            <label>Program path</label>
            <PathInput
              value={value.path}
              placeholder="C:\\Path\\To\\App.exe"
              title="Select an application or program"
              filters={[
                { name: "Programs", extensions: ["exe", "bat", "cmd", "lnk", "app"] },
                { name: "All files", extensions: ["*"] },
              ]}
              onChange={(p) => onChange({ ...value, path: p })}
            />
          </div>
          <div className="field">
            <label>Args (space-separated)</label>
            <input
              value={value.args.join(" ")}
              placeholder="optional"
              onChange={(e) =>
                onChange({
                  ...value,
                  args: e.target.value.split(/\s+/).filter(Boolean),
                })
              }
            />
          </div>
        </>
      )}

      {value?.type === "http_request" && (
        <>
          <div className="field-row">
            <div className="field field--xs">
              <label>Method</label>
              <select
                value={value.method}
                onChange={(e) => onChange({ ...value, method: e.target.value })}
              >
                <option value="POST">POST</option>
                <option value="GET">GET</option>
                <option value="PUT">PUT</option>
                <option value="DELETE">DELETE</option>
              </select>
            </div>
            <div className="field">
              <label>URL</label>
              <input
                value={value.url}
                placeholder="http://server:8787/run/restart-minecraft"
                onChange={(e) => onChange({ ...value, url: e.target.value })}
              />
            </div>
          </div>
          <div className="field-row">
            <div className="field field--sm">
              <label>Auth header</label>
              <input
                value={value.auth_header ?? ""}
                placeholder="X-Auth-Token"
                onChange={(e) => onChange({ ...value, auth_header: e.target.value || null })}
              />
            </div>
            <div className="field">
              <label>Auth token secret key</label>
              <input
                value={value.auth_token_secret_ref ?? ""}
                placeholder="e.g. webhook_token (store in Secrets)"
                onChange={(e) =>
                  onChange({ ...value, auth_token_secret_ref: e.target.value || null })
                }
              />
            </div>
          </div>
          <div className="field">
            <label>Body (optional)</label>
            <input
              value={value.body ?? ""}
              placeholder="optional request body"
              onChange={(e) => onChange({ ...value, body: e.target.value || null })}
            />
          </div>
          <p className="hint">
            Fires in the background (no browser). Store the token in Settings →
            Secrets and reference its key here; it's sent in the auth header.
          </p>
        </>
      )}

      {value?.type === "frontend" && (
        <div className="field">
          <label>App action</label>
          <select
            value={value.name}
            onChange={(e) => onChange({ ...value, name: e.target.value })}
          >
            {FRONTEND_NAMES.map((n) => (
              <option key={n.value} value={n.value}>
                {n.label}
              </option>
            ))}
          </select>
        </div>
      )}
    </div>
  );
}
