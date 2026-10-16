import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  value: string;
  placeholder?: string;
  onChange: (path: string) => void;
  /** Restrict the picker to certain extensions, e.g. executables or images. */
  filters?: { name: string; extensions: string[] }[];
  /** Dialog title. */
  title?: string;
  /** When true, opens a folder picker instead of a file picker. */
  directory?: boolean;
}

/**
 * A text input paired with a "Browse…" button that opens the native OS file
 * picker (Windows Explorer / macOS Finder) via the Tauri dialog plugin.
 */
export function PathInput({ value, placeholder, onChange, filters, title, directory }: Props) {
  const browse = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: directory ?? false,
        title: title ?? (directory ? "Select a folder" : "Select a file"),
        filters: directory ? undefined : filters,
      });
      // `open` returns a string path, or null if cancelled.
      if (typeof selected === "string") {
        onChange(selected);
      }
    } catch (e) {
      console.error("file picker failed", e);
    }
  };

  return (
    <div className="path-input">
      <input
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
      <button type="button" className="btn btn--ghost btn--xs" onClick={browse}>
        Browse…
      </button>
    </div>
  );
}
