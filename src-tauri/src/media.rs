//! System media (SMTC) reader + transport controls (Windows only).
//!
//! Uses the Windows "GlobalSystemMediaTransportControls" session manager to see
//! whatever is currently playing media — a browser playing YouTube, Tidal,
//! Spotify, etc. — read its now-playing title/artist/playback-state, and send
//! transport commands (play/pause toggle, next, previous).
//!
//! The WinRT async operations implement `IntoFuture` in the `windows` crate.
//! WinRT interface objects are NOT `Send`, so we must not hold them across an
//! `.await` in a `Send` async context (Tauri/tokio spawn + commands require
//! `Send`). We therefore run all SMTC work on a dedicated OS thread with its
//! own current-thread runtime and return only plain `Send` data. The public
//! API is sync.
//!
//! On non-Windows targets everything is a no-op stub.

/// A snapshot of the current media session.
#[derive(Debug, Clone, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    /// Source app id (e.g. a browser or Spotify), best-effort.
    pub app: String,
    /// True when the session is actively playing (vs paused/stopped).
    pub playing: bool,
    /// True when any media session exists at all (something is loaded).
    pub active: bool,
    /// Album/media artwork as a `data:` URI (base64), empty if none.
    pub art: String,
}

/// Read the current media session, if any.
pub fn now_playing() -> NowPlaying {
    #[cfg(target_os = "windows")]
    {
        // Isolate the non-Send WinRT work on its own thread + runtime.
        std::thread::spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => return NowPlaying::default(),
            };
            rt.block_on(async { win::now_playing().await.unwrap_or_default() })
        })
        .join()
        .unwrap_or_default()
    }
    #[cfg(not(target_os = "windows"))]
    {
        NowPlaying::default()
    }
}

/// Transport actions we support.
#[derive(Debug, Clone, Copy)]
pub enum MediaAction {
    Toggle,
    Next,
    Previous,
    Play,
    Pause,
}

/// Send a transport command to the current media session. Returns true if a
/// session accepted it.
pub fn control(action: MediaAction) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(_) => return false,
            };
            rt.block_on(async { win::control(action).await.unwrap_or(false) })
        })
        .join()
        .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = action;
        false
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::{MediaAction, NowPlaying};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    };

    /// Get the current session (the one with media focus), if any.
    async fn current_session() -> windows::core::Result<Session> {
        let mgr = SessionManager::RequestAsync()?.await?;
        mgr.GetCurrentSession()
    }

    pub async fn now_playing() -> Option<NowPlaying> {
        let session = current_session().await.ok()?;

        let mut np = NowPlaying {
            active: true,
            ..Default::default()
        };

        if let Ok(id) = session.SourceAppUserModelId() {
            np.app = id.to_string_lossy();
        }

        if let Ok(info) = session.GetPlaybackInfo() {
            if let Ok(status) = info.PlaybackStatus() {
                np.playing = status == PlaybackStatus::Playing;
            }
        }

        if let Ok(op) = session.TryGetMediaPropertiesAsync() {
            if let Ok(props) = op.await {
                if let Ok(t) = props.Title() {
                    np.title = t.to_string_lossy();
                }
                if let Ok(a) = props.Artist() {
                    np.artist = a.to_string_lossy();
                }
                // Album/media artwork. Reading + base64-encoding the thumbnail
                // every poll would be wasteful, so cache it keyed by title.
                np.art = artwork_for(&np.title, &props).await;
            }
        }

        Some(np)
    }

    /// Cache of the last encoded artwork, keyed by track title, so we only read
    /// and base64-encode the thumbnail when the track changes.
    fn art_cache() -> &'static std::sync::Mutex<Option<(String, String)>> {
        static C: std::sync::OnceLock<std::sync::Mutex<Option<(String, String)>>> =
            std::sync::OnceLock::new();
        C.get_or_init(|| std::sync::Mutex::new(None))
    }

    /// Read the thumbnail stream for the current track and return a `data:` URI.
    /// Returns the cached value when the title is unchanged.
    async fn artwork_for(
        title: &str,
        props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
    ) -> String {
        if !title.is_empty() {
            if let Some((t, uri)) = art_cache().lock().unwrap().as_ref() {
                if t == title {
                    return uri.clone();
                }
            }
        }
        let uri = read_thumbnail_data_uri(props).await.unwrap_or_default();
        if !title.is_empty() {
            *art_cache().lock().unwrap() = Some((title.to_string(), uri.clone()));
        }
        uri
    }

    /// Read the thumbnail bytes and encode as a base64 `data:` URI.
    async fn read_thumbnail_data_uri(
        props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
    ) -> Option<String> {
        use windows::Storage::Streams::{DataReader, InputStreamOptions};

        let thumb_ref = props.Thumbnail().ok()?;
        let stream = thumb_ref.OpenReadAsync().ok()?.await.ok()?;
        let size = stream.Size().ok()?;
        if size == 0 || size > 8 * 1024 * 1024 {
            return None; // empty or implausibly large
        }
        let content_type = stream.ContentType().map(|s| s.to_string_lossy()).unwrap_or_default();
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.SetInputStreamOptions(InputStreamOptions::None).ok();
        let loaded = reader.LoadAsync(size as u32).ok()?.await.ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;

        let mime = if content_type.is_empty() {
            "image/png".to_string()
        } else {
            content_type
        };
        Some(format!("data:{};base64,{}", mime, base64_encode(&bytes)))
    }

    /// Minimal base64 encoder (no external crate).
    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(T[((n >> 6) & 63) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(T[(n & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    pub async fn control(action: MediaAction) -> windows::core::Result<bool> {
        let session = match current_session().await {
            Ok(s) => s,
            Err(_) => return Ok(false),
        };
        let op = match action {
            MediaAction::Toggle => session.TryTogglePlayPauseAsync()?,
            MediaAction::Next => session.TrySkipNextAsync()?,
            MediaAction::Previous => session.TrySkipPreviousAsync()?,
            MediaAction::Play => session.TryPlayAsync()?,
            MediaAction::Pause => session.TryPauseAsync()?,
        };
        Ok(op.await.unwrap_or(false))
    }
}
