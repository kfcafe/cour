//! Bootstrap library crate for the `cour` client.
//!
//! Modules will be added here incrementally by later work units.

pub mod ai;
pub mod commands;
pub mod config;
pub mod error;
pub mod index;
pub mod ingest;
pub mod maildir;
pub mod model;
pub mod parse;
pub mod send;
pub mod sync;
pub mod threading;
pub mod ui;
pub mod watch;

/// Shared prelude placeholder for future library exports.
pub mod prelude {
    pub use crate::commands::{Cli, Commands};
    pub use crate::config::ProjectPaths;
    pub use crate::error::{AppError, AppResult};
}

#[cfg(test)]
pub mod test_support {
    use std::collections::HashMap;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    pub struct TestEnvGuard {
        _guard: MutexGuard<'static, ()>,
        originals: HashMap<String, Option<OsString>>,
    }

    impl TestEnvGuard {
        pub fn acquire() -> Self {
            Self {
                _guard: env_lock().lock().expect("lock test env"),
                originals: HashMap::new(),
            }
        }

        pub fn set_var<K, V>(&mut self, key: K, value: V)
        where
            K: AsRef<str>,
            V: AsRef<OsStr>,
        {
            let key = key.as_ref().to_string();
            self.capture_original(&key);
            std::env::set_var(&key, value.as_ref());
        }

        pub fn remove_var<K>(&mut self, key: K)
        where
            K: AsRef<str>,
        {
            let key = key.as_ref().to_string();
            self.capture_original(&key);
            std::env::remove_var(&key);
        }

        fn capture_original(&mut self, key: &str) {
            self.originals
                .entry(key.to_string())
                .or_insert_with(|| std::env::var_os(key));
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.originals.drain() {
                match value {
                    Some(value) => std::env::set_var(&key, value),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}
