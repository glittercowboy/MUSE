//! File watcher for live-preview hot-reload.
//!
//! Watches a `.muse` source file for changes using `notify-debouncer-mini`
//! with a ~300ms debounce window. Sends change events through an `mpsc` channel.
//! Handles atomic renames (vim, VSCode save-to-temp-then-rename pattern).

use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// A file-change event from the watcher.
#[derive(Debug)]
pub struct FileChanged {
    pub path: PathBuf,
}

/// Watches a single `.muse` source file for modifications.
///
/// The watcher runs on a background thread managed by `notify`. Change events
/// are debounced to ~300ms and sent through the returned `mpsc::Receiver`.
///
/// We watch the *parent directory* rather than the file itself so that atomic
/// renames (common in editors) are captured — the file inode changes, but the
/// directory's entry list changes and notify catches that.
pub struct FileWatcher {
    // Hold the debouncer to keep the background thread alive.
    // Drop stops watching.
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    pub rx: mpsc::Receiver<FileChanged>,
}

impl FileWatcher {
    /// Start watching `source_path` for changes.
    ///
    /// Returns a `FileWatcher` whose `.rx` field receives `FileChanged` events.
    /// The watcher thread runs until the `FileWatcher` is dropped.
    pub fn start(source_path: &Path) -> Result<Self, String> {
        let canonical = source_path
            .canonicalize()
            .map_err(|e| format!("cannot resolve path '{}': {e}", source_path.display()))?;

        let watch_dir = canonical
            .parent()
            .ok_or_else(|| format!("cannot determine parent directory of '{}'", canonical.display()))?
            .to_path_buf();

        let file_name = canonical
            .file_name()
            .ok_or_else(|| format!("cannot determine file name of '{}'", canonical.display()))?
            .to_os_string();

        let (tx, rx) = mpsc::channel::<FileChanged>();

        let target_name = file_name.clone();
        let target_path = canonical.clone();
        let watch_dir_for_closure = watch_dir.clone();

        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            move |result: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                let events = match result {
                    Ok(evts) => evts,
                    Err(e) => {
                        eprintln!("[muse preview] watcher error: {e}");
                        return;
                    }
                };

                // Filter for events that affect our target file.
                // Editors may fire events on temp files in the same directory —
                // we only care about the actual source file.
                let dominated = events.iter().any(|evt| {
                    // Accept direct modification of our file
                    if evt.path == target_path {
                        return true;
                    }
                    // Accept events on files with the same name (handles atomic rename)
                    if let Some(name) = evt.path.file_name() {
                        if name == target_name {
                            return true;
                        }
                    }
                    // Accept AnyContinuous events on the parent dir (rename completion)
                    if evt.kind == DebouncedEventKind::AnyContinuous
                        && evt.path == watch_dir_for_closure
                    {
                        return true;
                    }
                    false
                });

                if dominated {
                    // Best-effort send — if the receiver is gone, we're shutting down
                    let _ = tx.send(FileChanged {
                        path: target_path.clone(),
                    });
                }
            },
        )
        .map_err(|e| format!("failed to create file watcher: {e}"))?;

        debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
            .map_err(|e| format!("failed to watch '{}': {e}", watch_dir.display()))?;

        Ok(Self {
            _debouncer: debouncer,
            rx,
        })
    }
}
