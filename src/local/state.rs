//! Session metadata and core snapshots share one validated directory capability.
use super::Error;
use rustix::fs::{AtFlags, Mode, OFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Component, Path},
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

pub(super) fn private_directory(path: &Path) -> Result<(), Error> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    private_directory_metadata(&fs::symlink_metadata(path)?)
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
            .open(path)?;
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
            rustix::fs::mkdirat(&self.0, name, Mode::from_raw_mode(0o700))?;
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
