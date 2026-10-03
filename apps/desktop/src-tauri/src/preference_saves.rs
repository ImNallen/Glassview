use crate::{preferences::Preferences, state::report, Result};
use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

enum SaveRequest {
    Update(Preferences),
    Flush(Sender<Result<()>>),
}

/// One writer serializes background saves and the save on exit, so a slider drag
/// writes the file once rather than on every step.
pub(crate) struct PreferenceSaves(Sender<SaveRequest>);

impl PreferenceSaves {
    pub(crate) fn start(app: tauri::AppHandle) -> Self {
        let errors = app.clone();
        Self::spawn(
            move |preferences| preferences.save(&app),
            move |error| report(&errors, error),
        )
    }

    fn spawn(
        write: impl FnMut(&Preferences) -> Result<()> + Send + 'static,
        on_error: impl FnMut(String) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || save_loop(receiver, write, on_error));
        Self(sender)
    }

    pub(crate) fn queue(&self, preferences: Preferences) -> Result<()> {
        self.0
            .send(SaveRequest::Update(preferences))
            .map_err(|_| "Preference saving is unavailable".into())
    }

    pub(crate) fn flush(&self) -> Result<()> {
        let (sender, receiver) = mpsc::channel();
        self.0
            .send(SaveRequest::Flush(sender))
            .map_err(|_| "Preference saving is unavailable".to_string())?;
        receiver
            .recv()
            .map_err(|_| "Preference saving stopped".to_string())?
    }
}

fn save_loop(
    receiver: Receiver<SaveRequest>,
    mut write: impl FnMut(&Preferences) -> Result<()>,
    mut on_error: impl FnMut(String),
) {
    let mut pending = None;
    let mut dirty = false;
    loop {
        let request = if dirty {
            match receiver.recv_timeout(Duration::from_millis(120)) {
                Ok(request) => request,
                Err(RecvTimeoutError::Timeout) => {
                    // Keep a failed save for an explicit flush, without a retry loop.
                    if let Some(preferences) = pending.as_ref() {
                        match write(preferences) {
                            Ok(()) => pending = None,
                            Err(error) => on_error(format!("Could not save preferences: {error}")),
                        }
                    }
                    dirty = false;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(request) => request,
                Err(_) => break,
            }
        };
        match request {
            SaveRequest::Update(preferences) => {
                pending = Some(preferences);
                dirty = true;
            }
            SaveRequest::Flush(response) => {
                let result = pending.as_ref().map_or(Ok(()), &mut write);
                if result.is_ok() {
                    pending = None;
                }
                dirty = false;
                let _ = response.send(result);
            }
        }
    }
    if let Some(preferences) = pending {
        if let Err(error) = write(&preferences) {
            on_error(format!("Could not save preferences: {error}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyMode;
    use std::sync::{Arc, Mutex};

    fn with_keys(keys: KeyMode) -> Preferences {
        Preferences {
            keys,
            ..Preferences::default()
        }
    }

    #[test]
    fn updates_do_not_wait_for_a_slow_disk_write() {
        let (started, writing) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let saved = writes.clone();
        let mut first = true;
        let writer = PreferenceSaves::spawn(
            move |p| {
                if first {
                    first = false;
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                }
                saved.lock().unwrap().push(p.keys);
                Ok(())
            },
            |_| panic!("unexpected save failure"),
        );
        writer.queue(with_keys(KeyMode::Shortcuts)).unwrap();
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        writer.queue(with_keys(KeyMode::All)).unwrap();
        release.send(()).unwrap();
        writer.flush().unwrap();
        assert_eq!(*writes.lock().unwrap(), [KeyMode::Shortcuts, KeyMode::All]);
    }

    #[test]
    fn rapid_changes_coalesce_and_flush_preserves_the_latest() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let saved = writes.clone();
        let writer = PreferenceSaves::spawn(
            move |p| {
                saved.lock().unwrap().push(p.keys);
                Ok(())
            },
            |_| panic!("unexpected save failure"),
        );
        for keys in [KeyMode::All, KeyMode::Shortcuts, KeyMode::All] {
            writer.queue(with_keys(keys)).unwrap();
        }
        writer.flush().unwrap();
        assert_eq!(*writes.lock().unwrap(), [KeyMode::All]);
    }

    #[test]
    fn background_failure_is_reported_and_flush_retries_it() {
        let (errors, received) = mpsc::channel();
        let mut attempts = 0;
        let writer = PreferenceSaves::spawn(
            move |_| {
                attempts += 1;
                if attempts == 1 {
                    Err("disk full".into())
                } else {
                    Ok(())
                }
            },
            move |error| {
                errors.send(error).unwrap();
            },
        );
        writer.queue(Preferences::default()).unwrap();
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            "Could not save preferences: disk full"
        );
        writer.flush().unwrap();
    }
}
