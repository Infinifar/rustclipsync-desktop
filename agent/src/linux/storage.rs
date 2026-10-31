//! Private XDG storage. Missing/corrupt keys never recreate an existing identity.
use dbus_secret_service::{EncryptionType, SecretService};
use rustclipsync_core::{
    adapters::StateStore,
    state::{self, MAX_STATE},
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

pub fn data_dir() -> io::Result<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state").map(|p| p.join("rustclipsync-desktop"))
}
pub fn xdg(name: &str, fallback: &str) -> io::Result<PathBuf> {
    if let Some(value) = std::env::var_os(name).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(io::Error::other("XDG paths must be absolute"));
        }
        return Ok(path);
    }
    let home = PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME unavailable"))?,
    );
    if !home.is_absolute() {
        return Err(io::Error::other("HOME must be absolute"));
    }
    Ok(home.join(fallback))
}
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    // XDG parents may be shared with other applications; chmod only our directory.
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other(
            "private directory has invalid owner or type",
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
use std::os::unix::fs::DirBuilderExt;
pub fn reject_reparse(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && m.uid() == unsafe { libc::geteuid() } && m.nlink() == 1 => Ok(true),
        Ok(_) => Err(io::Error::other(
            "private storage refuses links and foreign owners",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn open_private(path: &Path, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(path)?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 {
        return Err(io::Error::other("private file has invalid owner or type"));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
pub fn read_private(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match open_private(path, false) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("private file too large"));
    }
    Ok(Some(bytes))
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    reject_reparse(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    let tmp = parent.join(format!(".{}.tmp", state::random_uuid()?));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
pub struct InstanceLock(File);
impl InstanceLock {
    pub fn acquire(dir: &Path) -> io::Result<Self> {
        let file = open_private(&dir.join("agent.lock"), true)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::other("this profile is already in use"));
        }
        Ok(Self(file))
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = self.0.sync_all();
    }
}
#[derive(Clone)]
pub struct FileStateStore(pub PathBuf);
impl StateStore for FileStateStore {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        read_private(&self.0, MAX_STATE)
    }
    fn save(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() as u64 > MAX_STATE {
            return Err(io::Error::other("state too large"));
        }
        atomic_write(&self.0, bytes)
    }
}
pub fn load_key(dir: &Path, identity: &str) -> io::Result<[u8; 32]> {
    let failure =
        |_| io::Error::other("Secret Service unavailable or locked; unlock the desktop keyring");
    let service = SecretService::connect(EncryptionType::Dh).map_err(failure)?;
    // Include the profile directory so temporary/test profiles never share production keys.
    let profile = rustclipsync_core::hash(&dir.to_string_lossy());
    let attrs = HashMap::from([
        ("application", "rustclipsync-desktop"),
        ("profile", profile.as_str()),
        ("identity", identity),
    ]);
    let found = service.search_items(attrs.clone()).map_err(failure)?;
    if !found.locked.is_empty() {
        return Err(failure(dbus_secret_service::Error::Locked));
    }
    if found.unlocked.len() > 1 {
        return Err(io::Error::other("ambiguous state key"));
    }
    if let Some(item) = found.unlocked.first() {
        let secret = zeroize::Zeroizing::new(item.get_secret().map_err(failure)?);
        return secret
            .as_slice()
            .try_into()
            .map_err(|_| io::Error::other("invalid state key"));
    }
    if reject_reparse(&dir.join(format!("{identity}.state")))?
        || reject_reparse(&dir.join(format!("{identity}.history")))?
    {
        return Err(io::Error::other(
            "encrypted data exists but its key is missing",
        ));
    }
    let key = state::random_key()?;
    let collection = service.get_default_collection().map_err(failure)?;
    if collection.is_locked().map_err(failure)? {
        return Err(io::Error::other("unlock the desktop keyring first"));
    }
    collection
        .create_item(
            "RustClipSync Desktop state key",
            attrs,
            &key,
            false,
            "application/octet-stream",
        )
        .map_err(failure)?;
    Ok(key)
}
