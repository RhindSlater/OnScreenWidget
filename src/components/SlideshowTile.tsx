import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../api";
import type { BorderEffect, Tile, TileStatus } from "../types";

interface Props {
  tile: Tile;
  status?: TileStatus;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
}

const VIDEO_EXTS = new Set(["mp4", "webm", "ogg", "mov", "mkv", "avi"]);

function extOf(path: string): string {
  const dot = path.lastIndexOf(".");
  return dot === -1 ? "" : path.slice(dot + 1).toLowerCase();
}

/**
 * Slideshow tile: cycles through images and videos in a local folder.
 * The backend provides the sorted file list; this component handles rotation.
 */
export function SlideshowTile({
  tile,
  status,
  borderEffect = "orbit",
  onPointerDown,
}: Props) {
  const [files, setFiles] = useState<string[]>([]);
  const [index, setIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const folder =
    tile.kind.type === "slideshow" ? tile.kind.folder : "";
  const intervalSecs =
    tile.kind.type === "slideshow" ? tile.kind.interval_secs : 30;

  // Fetch the file list whenever the folder changes.
  const fetchFiles = useCallback(() => {
    if (!folder) {
      setFiles([]);
      setError("no folder set");
      return;
    }
    api
      .listSlideshowFiles(folder)
      .then((result) => {
        setFiles(result);
        setIndex(0);
        setError(result.length === 0 ? "folder empty" : null);
      })
      .catch((e) => {
        setFiles([]);
        setError(String(e));
      });
  }, [folder]);

  useEffect(() => {
    fetchFiles();
  }, [fetchFiles]);

  // Rotate on a timer.
  useEffect(() => {
    if (timerRef.current) clearInterval(timerRef.current);
    if (files.length <= 1) return;
    timerRef.current = setInterval(() => {
      setIndex((i) => (i + 1) % files.length);
    }, intervalSecs * 1000);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [files.length, intervalSecs]);

  const health = status?.health ?? "unknown";
  const currentFile = files[index] ?? null;
  const isVideo = currentFile ? VIDEO_EXTS.has(extOf(currentFile)) : false;
  const src = currentFile ? convertFileSrc(currentFile) : "";

  const classes = [
    "tile",
    "tile--slideshow",
    `tile--${health}`,
    borderEffect !== "none" ? `tile--border-${borderEffect}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  // Grid footprint from tile.size.
  const [spanCols, spanRows] =
    tile.size === "2x2"
      ? [2, 2]
      : tile.size === "2x1"
        ? [2, 1]
        : tile.size === "1x2"
          ? [1, 2]
          : [1, 1];

  return (
    <div
      className={classes}
      data-tile-id={tile.id}
      style={{
        gridColumn: `span ${spanCols}`,
        gridRow: `span ${spanRows}`,
      }}
      onPointerDown={onPointerDown}
      title={status?.detail ?? (currentFile ?? "slideshow")}
    >
      {error || !currentFile ? (
        <div className="slideshow__empty">
          <span className="slideshow__icon">🖼️</span>
          <span className="slideshow__msg">
            {error ?? "loading…"}
          </span>
        </div>
      ) : isVideo ? (
        <video
          className="slideshow__media"
          key={currentFile}
          src={src}
          autoPlay
          muted
          loop
          playsInline
        />
      ) : (
        <img
          className="slideshow__media"
          key={currentFile}
          src={src}
          alt={tile.label}
          draggable={false}
        />
      )}
      <div className="slideshow__label">{tile.label}</div>
      {files.length > 1 && (
        <div className="slideshow__counter">
          {index + 1}/{files.length}
        </div>
      )}
    </div>
  );
}
