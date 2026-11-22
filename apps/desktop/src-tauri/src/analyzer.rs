//! Route the webview analyzer session to one native PCM source.

use std::sync::{Arc, Mutex};

use digi_coupled::pcm_observer::{PcmCursor, PcmSource, PcmWindow};
use tauri::State;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Source {
    Emulator,
    Remote,
}

struct Session {
    id: u32,
    source: Source,
    cursor: PcmCursor,
}

#[derive(Default)]
struct Sessions {
    next: u32,
    current: Option<Session>,
}

#[derive(Default)]
pub(crate) struct Analyzer {
    emulator: Arc<PcmSource>,
    remote: Arc<PcmSource>,
    sessions: Mutex<Sessions>,
}

impl Analyzer {
    pub(crate) fn source(&self, source: Source) -> Arc<PcmSource> {
        self.pcm_source(source).clone()
    }

    fn pcm_source(&self, source: Source) -> &Arc<PcmSource> {
        match source {
            Source::Emulator => &self.emulator,
            Source::Remote => &self.remote,
        }
    }

    fn open(&self, source: Source) -> Result<u32, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let id = sessions
            .next
            .checked_add(1)
            .ok_or("Analyzer session limit reached")?;
        sessions.next = id;
        if let Some(old) = sessions.current.take() {
            self.pcm_source(old.source).subscribe(0);
        }
        self.pcm_source(source).subscribe(id);
        sessions.current = Some(Session {
            id,
            source,
            cursor: PcmCursor::default(),
        });
        Ok(id)
    }

    fn read(&self, id: u32) -> Option<PcmWindow> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .current
            .as_mut()
            .filter(|session| session.id == id)?;
        self.pcm_source(session.source)
            .read(id, &mut session.cursor)
    }

    fn close(&self, id: u32) {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions
            .current
            .as_ref()
            .is_some_and(|session| session.id == id)
        {
            let session = sessions.current.take().unwrap();
            self.pcm_source(session.source).subscribe(0);
        }
    }
}

#[tauri::command]
pub(crate) async fn analyzer_open(
    state: State<'_, Analyzer>,
    source: Source,
) -> Result<u32, String> {
    state.open(source)
}

#[tauri::command]
pub(crate) async fn analyzer_read(
    state: State<'_, Analyzer>,
    id: u32,
) -> Result<Option<PcmWindow>, String> {
    Ok(state.read(id))
}

#[tauri::command]
pub(crate) async fn analyzer_close(state: State<'_, Analyzer>, id: u32) -> Result<(), String> {
    state.close(id);
    Ok(())
}

#[cfg(test)]
mod tests;
