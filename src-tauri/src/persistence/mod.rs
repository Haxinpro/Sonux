pub mod active;
pub mod aliases;
pub mod assignments;
pub mod autostart;
pub mod backup;
pub mod buses;
pub mod channels;
pub mod eq;
pub mod eq_presets;
pub mod mic;
pub mod mic_presets;
pub mod outputs;
pub mod prefs;
pub mod profile_automation;
pub mod profiles;
pub mod seen;
pub mod window;
pub mod wireplumber;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

thread_local! {
    static CONFIG_QUIESCE_OWNER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static CONFIG_WRITE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

struct ConfigWriteBarrier {
    lock: RwLock<()>,
    quiesced: AtomicBool,
}

impl ConfigWriteBarrier {
    fn shared(&self) -> std::io::Result<RwLockReadGuard<'_, ()>> {
        if self.quiesced.load(Ordering::Acquire) {
            return Err(std::io::Error::other(
                "configuration is quiesced while Sonux restarts",
            ));
        }
        let guard = self
            .lock
            .read()
            .map_err(|_| std::io::Error::other("configuration write barrier is poisoned"))?;
        if self.quiesced.load(Ordering::Acquire) {
            return Err(std::io::Error::other(
                "configuration is quiesced while Sonux restarts",
            ));
        }
        Ok(guard)
    }

    fn exclusive(&self) -> std::io::Result<RwLockWriteGuard<'_, ()>> {
        self.lock
            .write()
            .map_err(|_| std::io::Error::other("configuration write barrier is poisoned"))
    }
}

static CONFIG_WRITES: LazyLock<ConfigWriteBarrier> = LazyLock::new(|| ConfigWriteBarrier {
    lock: RwLock::new(()),
    quiesced: AtomicBool::new(false),
});

pub(crate) enum ConfigWritePermit {
    Shared(#[allow(dead_code)] RwLockReadGuard<'static, ()>),
    Nested,
    QuiesceOwner,
}

impl Drop for ConfigWritePermit {
    fn drop(&mut self) {
        if matches!(self, Self::Shared(_) | Self::Nested) {
            CONFIG_WRITE_DEPTH.set(CONFIG_WRITE_DEPTH.get().saturating_sub(1));
        }
    }
}

pub(crate) fn begin_config_write() -> std::io::Result<ConfigWritePermit> {
    if CONFIG_QUIESCE_OWNER.get() {
        return Ok(ConfigWritePermit::QuiesceOwner);
    }
    if CONFIG_WRITE_DEPTH.get() > 0 {
        CONFIG_WRITE_DEPTH.set(CONFIG_WRITE_DEPTH.get() + 1);
        return Ok(ConfigWritePermit::Nested);
    }
    let guard = CONFIG_WRITES.shared()?;
    CONFIG_WRITE_DEPTH.set(1);
    Ok(ConfigWritePermit::Shared(guard))
}

pub struct ConfigQuiesceGuard {
    guard: Option<RwLockWriteGuard<'static, ()>>,
    committed: bool,
}

pub struct ConfigSnapshotGuard {
    #[allow(dead_code)]
    guard: RwLockWriteGuard<'static, ()>,
}

/// Wait for in-flight configuration writers and exclude new ones for the
/// lifetime of the guard, without changing the permanent quiescence flag.
pub fn lock_config_snapshot() -> std::io::Result<ConfigSnapshotGuard> {
    Ok(ConfigSnapshotGuard {
        guard: CONFIG_WRITES.exclusive()?,
    })
}

impl ConfigQuiesceGuard {
    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ConfigQuiesceGuard {
    fn drop(&mut self) {
        if !self.committed {
            CONFIG_WRITES.quiesced.store(false, Ordering::Release);
        }
        self.guard.take();
        CONFIG_QUIESCE_OWNER.set(false);
    }
}

/// Wait for every in-flight configuration writer, then reject all future
/// writes until the current process exits. Call `commit` once destructive
/// replacement has begun; dropping an uncommitted guard reopens writes.
pub fn quiesce_config_writes() -> std::io::Result<ConfigQuiesceGuard> {
    let guard = CONFIG_WRITES.exclusive()?;
    CONFIG_QUIESCE_OWNER.set(true);
    CONFIG_WRITES.quiesced.store(true, Ordering::Release);
    Ok(ConfigQuiesceGuard {
        guard: Some(guard),
        committed: false,
    })
}

pub fn config_writes_quiesced() -> bool {
    CONFIG_WRITES.quiesced.load(Ordering::Acquire)
}

/// Seconds since the Unix epoch, or 0 if the clock predates it.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Move settings written by the pre-Sonux namespace into the current config
/// directory. Never merge into or overwrite an existing Sonux directory.
fn migrate_legacy_config_dir_from(base: &std::path::Path) -> std::io::Result<()> {
    let legacy = base.join("sink");
    let current = base.join("sonux");
    if legacy.exists() && !current.exists() {
        std::fs::rename(legacy, current)?;
    }
    Ok(())
}

pub fn migrate_legacy_config_dir() -> std::io::Result<()> {
    let Some(base) = dirs::config_dir() else {
        return Ok(());
    };
    migrate_legacy_config_dir_from(&base)
}

/// Create Sonux's config directory (and parents) with owner-only access -
/// routing rules and app history are nobody else's business. Used by every
/// save path that writes under `$XDG_CONFIG_HOME/sonux`.
pub fn ensure_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    let _write = begin_config_write()?;
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write `contents` to `path` atomically: write a sibling temp file, fsync it,
/// then rename it over the target. A crash or power loss mid-write then leaves
/// either the old file or the complete new one - never a truncated file that
/// load paths silently discard (resetting the user's config). The parent
/// directory is created if missing; callers needing 0700 call
/// [`ensure_private_dir`] first, which this preserves.
pub fn write_atomic(path: &std::path::Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let _write = begin_config_write()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Temp file in the same directory so the rename stays on one filesystem
    // (a cross-device rename is not atomic). Each writer needs its own file:
    // several command threads may persist independent settings concurrently.
    let (tmp, mut file) = loop {
        let mut candidate = path.as_os_str().to_owned();
        candidate.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let candidate = std::path::PathBuf::from(candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        // fsyncing the file makes its contents durable; fsyncing the parent
        // makes the rename itself durable across sudden power loss.
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn remove_file(path: &std::path::Path) -> std::io::Result<()> {
    let _write = begin_config_write()?;
    std::fs::remove_file(path)
}

/// Factory reset: delete everything Sonux or its legacy namespace ever saved - the whole config
/// directory (channels, mixes, profiles, assignments, history, prefs)
/// and the WirePlumber routing rules.
pub fn wipe_all() -> Result<(), crate::error::SinkError> {
    if let Some(dir) = dirs::config_dir() {
        for name in ["sonux", "sink"] {
            let app_dir = dir.join(name);
            if app_dir.exists() {
                std::fs::remove_dir_all(&app_dir)?;
            }
        }
    }
    if let Ok(conf) = wireplumber::conf_path() {
        if conf.exists() {
            std::fs::remove_file(&conf)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_overwrites_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!(
            "sink-write-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = dir.join("cfg.json");

        write_atomic(&path, b"first").expect("first write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        // A shorter follow-up must fully replace, not overlay, the old bytes.
        write_atomic(&path, b"second, longer contents").expect("overwrite");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "second, longer contents"
        );

        assert!(
            std::fs::read_dir(&dir).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            }),
            "temp file must not linger"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_atomic_writers_do_not_share_a_temp_file() {
        let dir = std::env::temp_dir().join(format!(
            "sonux-concurrent-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let path = std::sync::Arc::new(dir.join("cfg.json"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let payloads = (0..8)
            .map(|index| format!("writer-{index}:{}", "x".repeat(32 * 1024)))
            .collect::<Vec<_>>();

        let handles = payloads
            .iter()
            .cloned()
            .map(|payload| {
                let path = std::sync::Arc::clone(&path);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    write_atomic(&path, payload)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap().expect("concurrent write");
        }
        let saved = std::fs::read_to_string(&*path).unwrap();
        assert!(payloads.contains(&saved));
        assert!(std::fs::read_dir(&dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exclusive_barrier_waits_for_an_in_flight_writer() {
        let barrier = std::sync::Arc::new(ConfigWriteBarrier {
            lock: RwLock::new(()),
            quiesced: AtomicBool::new(false),
        });
        let writer = barrier.shared().unwrap();
        let other = std::sync::Arc::clone(&barrier);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _exclusive = other.exclusive().unwrap();
            other.quiesced.store(true, Ordering::Release);
            acquired_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(acquired_rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        drop(writer);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        handle.join().unwrap();
        assert!(barrier.quiesced.load(Ordering::Acquire));
        assert!(barrier.shared().is_err());
    }

    #[test]
    fn snapshot_exclusion_is_temporary_and_waits_for_writer() {
        let barrier = std::sync::Arc::new(ConfigWriteBarrier {
            lock: RwLock::new(()),
            quiesced: AtomicBool::new(false),
        });
        let writer = barrier.shared().unwrap();
        let other = std::sync::Arc::clone(&barrier);
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let exclusive = other.exclusive().unwrap();
            acquired_tx.send(()).unwrap();
            drop(exclusive);
        });
        assert!(acquired_rx
            .recv_timeout(std::time::Duration::from_millis(25))
            .is_err());
        drop(writer);
        acquired_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        handle.join().unwrap();
        assert!(!barrier.quiesced.load(Ordering::Acquire));
        assert!(barrier.shared().is_ok());
    }

    #[test]
    fn legacy_config_migrates_without_overwriting_current_state() {
        let dir = std::env::temp_dir().join(format!(
            "sonux-config-migration-{}-{}",
            std::process::id(),
            unix_now()
        ));
        let legacy = dir.join("sink");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("prefs.json"), b"legacy").unwrap();

        migrate_legacy_config_dir_from(&dir).unwrap();
        assert_eq!(
            std::fs::read(dir.join("sonux/prefs.json")).unwrap(),
            b"legacy"
        );
        assert!(!legacy.exists());

        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("prefs.json"), b"do not merge").unwrap();
        migrate_legacy_config_dir_from(&dir).unwrap();
        assert_eq!(
            std::fs::read(dir.join("sonux/prefs.json")).unwrap(),
            b"legacy"
        );
        assert!(legacy.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
