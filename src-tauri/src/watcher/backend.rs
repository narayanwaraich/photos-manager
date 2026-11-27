use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::RootId;
use crate::watcher::debounce::classify_notify_event;

/// Folded watcher signal produced by a backend callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEvent {
    Changed,
    Error,
}

pub type WatchCallback = Arc<dyn Fn(WatchEvent) + Send + Sync>;

/// Abstraction over filesystem watching backends so `WatcherManager` can be tested with a fake.
pub trait WatchBackend: Send + Sync {
    fn start_watch(
        &self,
        root_id: &RootId,
        path: &Path,
        callback: WatchCallback,
    ) -> Result<(), String>;
    fn stop_watch(&self, root_id: &RootId);
}

/// Production backend backed by `notify::RecommendedWatcher` (recursive).
#[derive(Default)]
pub struct NotifyWatchBackend {
    watchers: Mutex<HashMap<RootId, RecommendedWatcher>>,
}

impl NotifyWatchBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WatchBackend for NotifyWatchBackend {
    fn start_watch(
        &self,
        root_id: &RootId,
        path: &Path,
        callback: WatchCallback,
    ) -> Result<(), String> {
        let root_path = path.to_path_buf();
        let cb = Arc::clone(&callback);
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
                Ok(event) => {
                    if classify_notify_event(Some(&root_path), &event) {
                        cb(WatchEvent::Changed);
                    }
                }
                Err(_) => {
                    cb(WatchEvent::Error);
                }
            })
            .map_err(|e| e.to_string())?;

        watcher
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;

        let mut guard = match self.watchers.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.insert(root_id.clone(), watcher);
        Ok(())
    }

    fn stop_watch(&self, root_id: &RootId) {
        let removed = {
            let mut guard = match self.watchers.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            guard.remove(root_id)
        };
        // Drop the RecommendedWatcher outside the lock so OS stream teardown never blocks other roots.
        drop(removed);
    }
}
