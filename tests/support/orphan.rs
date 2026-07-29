use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    process::Child,
    sync::mpsc,
    thread,
    time::Duration,
};

use allez::ephemeral::EnvironmentId;
use fs4::FileExt;

pub(crate) const METADATA_FILE: &str = ".metadata.json";
pub(crate) const OWNER_LOCK_FILE: &str = ".owner.lock";
pub(crate) const ROOT_LOCK_FILE: &str = ".root.lock";

pub(crate) fn create_orphan_candidate(
    root: &Path,
    id: EnvironmentId,
    packages: &[&str],
) -> PathBuf {
    let location = root.join("envs").join(id.to_string());
    std::fs::create_dir(&location).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(&location, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    File::create(location.join(OWNER_LOCK_FILE)).unwrap();
    let metadata = serde_json::json!({
        "pid": std::process::id(),
        "created_at": 1,
        "environment_id": id.to_string(),
        "packages": packages,
    });
    std::fs::write(
        location.join(METADATA_FILE),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    location
}

pub(crate) struct HeldFileLock {
    release: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HeldFileLock {
    pub(crate) fn acquire(path: PathBuf) -> Self {
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .map_err(|error| error.to_string())
                .and_then(|file| {
                    FileExt::try_lock(&file)
                        .map_err(|error| format!("{error:?}"))
                        .map(|()| file)
                });
            match result {
                Ok(file) => {
                    ready_sender.send(Ok(())).unwrap();
                    let _released = release_receiver.recv();
                    drop(file);
                }
                Err(error) => ready_sender.send(Err(error)).unwrap(),
            }
        });
        ready_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        Self {
            release: Some(release_sender),
            thread: Some(thread),
        }
    }
}

impl Drop for HeldFileLock {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _result = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _result = thread.join();
        }
    }
}

pub(crate) struct ChildGuard(Option<Child>);

impl ChildGuard {
    pub(crate) fn new(child: Child) -> Self {
        Self(Some(child))
    }

    pub(crate) fn kill_and_wait(mut self) {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        let _status = child.wait().unwrap();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _kill_result = child.kill();
            let _wait_result = child.wait();
        }
    }
}

pub(crate) fn wait_for_file(path: &Path) {
    for _ in 0..500 {
        if path.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "helper process did not signal readiness: {}",
        path.display()
    );
}
