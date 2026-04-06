use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::AccountConfig;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct WatchAccumulator {
    debounce: Duration,
    seen: HashMap<PathBuf, Instant>,
}

impl WatchAccumulator {
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            seen: HashMap::new(),
        }
    }

    pub fn record_path(&mut self, path: PathBuf, now: Instant) {
        self.seen.insert(path, now);
    }

    pub fn drain_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        self.seen.retain(|path, seen_at| {
            if now.duration_since(*seen_at) >= self.debounce {
                ready.push(path.clone());
                false
            } else {
                true
            }
        });
        ready.sort();
        ready
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchPlan {
    pub account_name: String,
    pub root_path: PathBuf,
}

pub struct MaildirWatcher {
    _watcher: RecommendedWatcher,
    accumulator: Arc<Mutex<WatchAccumulator>>,
}

pub fn build_watch_plans(accounts: &[AccountConfig], watch_enabled: bool) -> Vec<WatchPlan> {
    if !watch_enabled {
        return Vec::new();
    }

    let mut plans: Vec<_> = accounts
        .iter()
        .map(|account| WatchPlan {
            account_name: account.name.clone(),
            root_path: account.maildir_root.clone(),
        })
        .collect();
    plans.sort_by(|left, right| left.account_name.cmp(&right.account_name));
    plans
}

impl MaildirWatcher {
    pub fn new(debounce: Duration) -> notify::Result<Self> {
        let accumulator = Arc::new(Mutex::new(WatchAccumulator::new(debounce)));
        let handle = Arc::clone(&accumulator);

        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            if let Ok(event) = result {
                let now = Instant::now();
                if let Ok(mut guard) = handle.lock() {
                    for path in event.paths {
                        guard.record_path(path, now);
                    }
                }
            }
        })?;

        Ok(Self {
            _watcher: watcher,
            accumulator,
        })
    }

    pub fn watch_path(&mut self, path: &Path) -> notify::Result<()> {
        self._watcher.watch(path, RecursiveMode::Recursive)
    }

    pub fn drain_ready(&self, now: Instant) -> Vec<PathBuf> {
        self.accumulator
            .lock()
            .map(|mut guard| guard.drain_ready(now))
            .unwrap_or_default()
    }
}

pub fn start_maildir_watcher(
    accounts: &[AccountConfig],
    watch_enabled: bool,
    debounce: Duration,
) -> AppResult<Option<MaildirWatcher>> {
    let plans = build_watch_plans(accounts, watch_enabled);
    if plans.is_empty() {
        return Ok(None);
    }

    let mut watcher = MaildirWatcher::new(debounce)
        .map_err(|err| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, err)))?;

    for plan in plans {
        watcher
            .watch_path(&plan.root_path)
            .map_err(|err| AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, err)))?;
    }

    Ok(Some(watcher))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crate::config::AccountConfig;

    use super::{build_watch_plans, WatchAccumulator};

    #[test]
    fn debounce_collapses_repeated_paths() {
        let start = Instant::now();
        let mut accumulator = WatchAccumulator::new(Duration::from_millis(50));
        let path = PathBuf::from("/tmp/mail/cur/msg-1");

        accumulator.record_path(path.clone(), start);
        accumulator.record_path(path.clone(), start + Duration::from_millis(10));

        let early = accumulator.drain_ready(start + Duration::from_millis(40));
        assert!(early.is_empty());

        let ready = accumulator.drain_ready(start + Duration::from_millis(61));
        assert_eq!(ready, vec![path]);
    }

    #[test]
    fn build_watch_plans_respects_disabled_flag() {
        let accounts = vec![AccountConfig {
            name: "personal".to_string(),
            email_address: "ash@example.com".to_string(),
            maildir_root: "/tmp/mail/personal".into(),
            sync_command: None,
            default: Some(true),
        }];

        assert!(build_watch_plans(&accounts, false).is_empty());
        assert_eq!(
            build_watch_plans(&accounts, true)[0].account_name,
            "personal"
        );
    }
}
