import { api } from "../api";
import type { BorderEffect, Tile, TileStatus } from "../types";

interface Props {
  tile: Tile;
  status?: TileStatus;
  borderEffect?: BorderEffect;
  onPointerDown?: (e: React.PointerEvent) => void;
}

/**
 * System media tile (Windows SMTC). Shows the current now-playing title/artist
 * from whatever is playing (YouTube/Tidal/Spotify/…), with transport controls.
 * The tile glows purple while something is actively playing.
 */
export function MediaTile({ tile, status, borderEffect = "orbit", onPointerDown }: Props) {
  const media = status?.media ?? null;
  const playing = media?.playing ?? false;
  const active = media?.active ?? false;

  const title = media?.title?.trim() || (active ? "Playing" : "Nothing playing");
  const artist = media?.artist?.trim() || "";
  const art = media?.art?.trim() || "";

  const classes = [
    "tile",
    "tile--media",
    playing ? "tile--media-playing" : "",
    borderEffect !== "none" ? `tile--border-${borderEffect}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  // Stop clicks on buttons from bubbling to the tile's drag/click handlers.
  const stop = (e: React.MouseEvent) => e.stopPropagation();

  return (
    <div
      className={classes}
      data-tile-id={tile.id}
      onPointerDown={onPointerDown}
      title={active ? `${title}${artist ? " — " + artist : ""}` : "Nothing playing"}
    >
      {art && (
        <div
          className="media__art"
          style={{ backgroundImage: `url("${art}")` }}
          aria-hidden
        />
      )}
      <div className="media__now">
        <div className="media__title">{title}</div>
        {artist && <div className="media__artist">{artist}</div>}
      </div>
      <div className="media__controls" onPointerDown={(e) => e.stopPropagation()}>
        <button
          type="button"
          className="media__btn"
          title="Previous"
          onClick={(e) => {
            stop(e);
            void api.mediaControl("previous");
          }}
        >
          ⏮
        </button>
        <button
          type="button"
          className="media__btn media__btn--primary"
          title={playing ? "Pause" : "Play"}
          onClick={(e) => {
            stop(e);
            void api.mediaControl("toggle");
          }}
        >
          {playing ? "⏸" : "▶"}
        </button>
        <button
          type="button"
          className="media__btn"
          title="Next"
          onClick={(e) => {
            stop(e);
            void api.mediaControl("next");
          }}
        >
          ⏭
        </button>
      </div>
    </div>
  );
}
