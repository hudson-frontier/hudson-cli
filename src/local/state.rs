//! Session metadata and core snapshots share one validated directory capability.
use super::Error;
use rustix::fs::{AtFlags, Mode, OFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

fn private_directory_metadata(metadata: &fs::Metadata) -> Result<(), Error> {
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err("local state directories must be user-owned directories with mode 0700, without symbolic links".into());
    }
    Ok(())
}

fn directory_path(path: &Path) -> PathBuf {
    // Trailing separators or `/.` make no-follow checks resolve through a
    // final symlink. Strip those lexical components, retaining ParentDir and
    // ancestor aliases; canonicalization would follow the link before checking.
    path.components().collect()
}

pub(super) fn private_directory(path: &Path) -> Result<Directory, Error> {
    let path = directory_path(path);
    Directory::from_file(durable_directory(&path, true, 0)?)
}

fn sync_directory_link(directory: &File) -> Result<(), Error> {
    directory.sync_all()?;
    // Resolve the actual parent from the pinned directory, including after a
    // rename. Syncing the child alone does not persist its entry in the parent.
    let parent = File::from(rustix::fs::openat(
        directory,
        "..",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    parent.sync_all()?;
    Ok(())
}

fn create_directory_in(
    parent: &File,
    name: &std::ffi::OsStr,
    existing: bool,
) -> Result<File, Error> {
    match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
        Ok(()) => (),
        Err(rustix::io::Errno::EXIST) if existing => (),
        Err(error) => return Err(error.into()),
    }
    let child = File::from(rustix::fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    private_directory_metadata(&child.metadata()?)?;
    child.sync_all()?;
    parent.sync_all()?;
    Ok(child)
}

fn durable_directory(path: &Path, private: bool, depth: usize) -> Result<File, Error> {
    if depth > 128 {
        return Err("local state path requires too many new directories".into());
    }
    // Existing ancestor aliases (for example macOS /var) remain supported.
    // The selected root and every newly created component are no-follow.
    let opened = if private {
        Directory::open(path).map(|directory| directory.0)
    } else {
        OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::NONBLOCK).bits() as i32)
            .open(path)
            .map_err(Error::from)
    };
    match opened {
        Ok(directory) => {
            // Also cover retry after a previous creation's parent sync failed:
            // existence does not establish that this link was made durable.
            sync_directory_link(&directory)?;
            Ok(directory)
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            let parent_path = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent = durable_directory(parent_path, false, depth + 1)?;
            match path.components().next_back() {
                Some(Component::Normal(name)) => create_directory_in(&parent, name, true),
                Some(Component::ParentDir) => {
                    let directory = File::from(rustix::fs::openat(
                        &parent,
                        "..",
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )?);
                    if private {
                        private_directory_metadata(&directory.metadata()?)?;
                    }
                    sync_directory_link(&directory)?;
                    Ok(directory)
                }
                _ => Err("local state path requires a directory name".into()),
            }
        }
        Err(error) => Err(error),
    }
}

fn private_file_metadata(metadata: &fs::Metadata) -> Result<(), Error> {
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(
            "local session files must be user-owned regular files with mode 0600, without links"
                .into(),
        );
    }
    Ok(())
}

fn filename(name: &str) -> Result<(), Error> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || name.contains('/')
        || name.as_bytes().contains(&0)
    {
        return Err("local state requires one filename component".into());
    }
    Ok(())
}

pub(super) struct Directory(File);
impl Directory {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
            .open(directory_path(path))?;
        Self::from_file(file)
    }

    fn from_file(file: File) -> Result<Self, Error> {
        private_directory_metadata(&file.metadata()?)?;
        Ok(Self(file))
    }

    pub fn file(&self) -> &File {
        &self.0
    }

    pub fn child(&self, name: &str, create: bool) -> Result<Self, Error> {
        filename(name)?;
        if create {
            return Self::from_file(create_directory_in(&self.0, name.as_ref(), false)?);
        }
        let file = rustix::fs::openat(
            &self.0,
            name,
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        Self::from_file(file.into())
    }

    fn open_file(&self, name: &str) -> std::io::Result<File> {
        filename(name).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        rustix::fs::openat(
            &self.0,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(std::io::Error::from)
    }

    pub fn require_file(&self, name: &str) -> Result<(), Error> {
        private_file_metadata(&self.open_file(name)?.metadata()?)
    }

    pub fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, Error> {
        let file = self.open_file(name)?;
        private_file_metadata(&file.metadata()?)?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err("local session metadata exceeds its size limit".into());
        }
        Ok(bytes)
    }

    pub fn replace(&self, name: &str, bytes: &[u8]) -> Result<(), Error> {
        filename(name)?;
        private_directory_metadata(&self.0.metadata()?)?;
        match self.open_file(name) {
            Ok(file) => private_file_metadata(&file.metadata()?)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let temporary = format!(".session-{}.tmp", uuid::Uuid::new_v4());
        let file = rustix::fs::openat(
            &self.0,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        let mut staged = Staged {
            directory: &self.0,
            name: temporary,
            published: false,
        };
        let mut file = File::from(file);
        file.write_all(bytes)?;
        file.sync_all()?;
        rustix::fs::renameat(&self.0, staged.name.as_str(), &self.0, name)?;
        staged.published = true;
        self.0.sync_all()?;
        Ok(())
    }
}

struct Staged<'a> {
    directory: &'a File,
    name: String,
    published: bool,
}
impl Drop for Staged<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _ = rustix::fs::unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn temporary() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    #[test]
    fn nested_state_creation_returns_the_pinned_private_directory() {
        let temporary = temporary();
        let root = temporary.path().join(".local/state/hudson");
        let directory = private_directory(&root).unwrap();
        for relative in [".local", ".local/state", ".local/state/hudson"] {
            assert_eq!(
                fs::metadata(temporary.path().join(relative))
                    .unwrap()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let outside = temporary.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::rename(
            temporary.path().join(".local"),
            temporary.path().join("moved"),
        )
        .unwrap();
        symlink(&outside, temporary.path().join(".local")).unwrap();
        let session = directory.child("session", true).unwrap();
        session.replace("session.json", b"private session").unwrap();
        assert_eq!(
            fs::read(
                temporary
                    .path()
                    .join("moved/state/hudson/session/session.json")
            )
            .unwrap(),
            b"private session"
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn ancestor_aliases_and_parent_components_keep_their_path_semantics() {
        let temporary = temporary();
        let real = temporary.path().join("real");
        private_directory(&real).unwrap();
        let alias = temporary.path().join("alias");
        symlink(&real, &alias).unwrap();
        let root = alias.join("made/../state/hudson");
        let directory = private_directory(&root).unwrap();
        directory.child("session", true).unwrap();
        assert!(real.join("made").is_dir());
        assert!(real.join("state/hudson/session").is_dir());
        assert!(private_directory(&alias).is_err());
        let reopened = private_directory(&root).unwrap();
        assert!(reopened.child("session", false).is_ok());
        assert!(reopened.child("session", true).is_err());
    }

    #[test]
    fn excessive_missing_ancestors_fail_before_creating_any_directory() {
        let temporary = temporary();
        let mut root = temporary.path().to_path_buf();
        for _ in 0..130 {
            root.push("new");
        }
        assert!(private_directory(&root).is_err());
        assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn session_creation_fails_when_its_parent_cannot_be_synced() {
        let temporary = temporary();
        let descriptor = rustix::fs::open(
            temporary.path(),
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .unwrap();
        let parent = Directory::from_file(descriptor.into()).unwrap();
        // O_PATH permits mkdirat/openat, but cannot acknowledge fsync. A new
        // session must not be usable after that durability gate fails.
        assert!(parent.child("session", true).is_err());
        assert!(temporary.path().join("session").is_dir());
        assert!(!temporary.path().join("session/session.json").exists());
        // A retry for root creation must acknowledge the same parent sync even
        // though its first attempt already left the directory entry present.
        assert!(create_directory_in(parent.file(), "state".as_ref(), true).is_err());
        assert!(create_directory_in(parent.file(), "state".as_ref(), true).is_err());
        let recovered = private_directory(&temporary.path().join("state")).unwrap();
        assert!(recovered.child("session", true).is_ok());
    }

    #[test]
    fn final_state_aliases_are_rejected_with_trailing_separators_or_dot() {
        let temporary = tempfile::tempdir().unwrap();
        let real = temporary.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(real.join("untouched"), b"preserve me").unwrap();
        let alias = temporary.path().join("alias");
        symlink(&real, &alias).unwrap();
        for suffix in ["", "/", "/.", "//./"] {
            let mut name = alias.as_os_str().to_os_string();
            name.push(suffix);
            let path = std::path::PathBuf::from(name);
            assert!(private_directory(&path).is_err(), "accepted alias{suffix}");
            assert!(Directory::open(&path).is_err(), "opened alias{suffix}");

            let mut name = real.as_os_str().to_os_string();
            name.push(suffix);
            let path = std::path::PathBuf::from(name);
            private_directory(&path).unwrap();
            Directory::open(&path).unwrap();
        }
        assert_eq!(fs::read(real.join("untouched")).unwrap(), b"preserve me");
        assert_eq!(fs::read_dir(real).unwrap().count(), 1);
    }

    #[test]
    fn ancestor_aliases_and_parent_directory_components_remain_supported() {
        let temporary = tempfile::tempdir().unwrap();
        let real = temporary.path().join("real");
        private_directory(&real).unwrap();
        private_directory(&real.join("child")).unwrap();
        let alias = temporary.path().join("alias");
        symlink(&real, &alias).unwrap();
        for path in [alias.join("./child/../child/."), alias.join("child/..")] {
            private_directory(&path).unwrap();
            let directory = Directory::open(&path).unwrap();
            assert!(directory.0.metadata().unwrap().is_dir());
        }
    }
}
