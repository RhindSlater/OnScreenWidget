import { useState } from "react";
import { api } from "../api";

/**
 * Store/clear secrets in the OS keychain by a `secret_ref` key. Values are
 * write-only here — we never read them back, only check existence.
 */
export function SecretsManager() {
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [checkKey, setCheckKey] = useState("");
  const [checkResult, setCheckResult] = useState<string | null>(null);

  const save = async () => {
    if (!key.trim() || !value) {
      setStatus("Enter both a key and a value.");
      return;
    }
    try {
      await api.setSecret(key.trim(), value);
      setStatus(`Saved "${key.trim()}" to the OS keychain.`);
      setValue("");
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    }
  };

  const remove = async () => {
    if (!key.trim()) {
      setStatus("Enter a key to delete.");
      return;
    }
    try {
      await api.deleteSecret(key.trim());
      setStatus(`Deleted "${key.trim()}" (if it existed).`);
    } catch (e) {
      setStatus(`Error: ${String(e)}`);
    }
  };

  const check = async () => {
    if (!checkKey.trim()) return;
    try {
      const exists = await api.hasSecret(checkKey.trim());
      setCheckResult(exists ? "✓ stored" : "✗ not found");
    } catch (e) {
      setCheckResult(`Error: ${String(e)}`);
    }
  };

  return (
    <div className="secrets">
      <p className="hint">
        Store API tokens / passwords here. They go into the OS keychain (never
        the config file). Reference the <em>key</em> name from a tile's secret
        field (e.g. <code>twitch_token</code>).
      </p>

      <div className="field">
        <label>Secret key</label>
        <input
          value={key}
          placeholder="twitch_token"
          onChange={(e) => setKey(e.target.value)}
        />
      </div>
      <div className="field">
        <label>Value</label>
        <input
          type="password"
          value={value}
          placeholder="paste token / password"
          onChange={(e) => setValue(e.target.value)}
        />
      </div>
      <div className="editor__actions">
        <button className="btn btn--danger" onClick={remove}>
          Delete
        </button>
        <button className="btn" onClick={save}>
          Save secret
        </button>
      </div>
      {status && <div className="note">{status}</div>}

      <div className="secrets__check">
        <div className="field">
          <label>Check a key exists</label>
          <input
            value={checkKey}
            placeholder="twitch_token"
            onChange={(e) => {
              setCheckKey(e.target.value);
              setCheckResult(null);
            }}
          />
        </div>
        <button className="btn btn--ghost" onClick={check}>
          Check
        </button>
        {checkResult && <span className="note">{checkResult}</span>}
      </div>
    </div>
  );
}
