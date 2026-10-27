use super::{Event, Sender};
use crate::local_args::Permission;
use hudson_core::{
    adapters::tools::{ExecutionError, Invocation, ToolExecutor},
    models::*,
};
use serde_json::{json, Value};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const FILE_LIMIT: usize = 64 * 1024;
const OUTPUT_LIMIT: usize = 24 * 1024;
const INTERNAL_EXEC: &str = "__hudson-internal-local-exec-v1";

/// Run before parsing or starting application threads. Successful exec never returns.
pub(crate) fn dispatch_internal(arguments: &[OsString]) -> Option<std::io::Error> {
    if arguments.first().map(OsString::as_os_str) != Some(OsStr::new(INTERNAL_EXEC)) {
        return None;
    }
    let Some(program) = arguments.get(1) else {
        return Some(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "internal process target missing",
        ));
    };
    if let Err(error) = rustix::process::fchdir(std::io::stdin()) {
        return Some(error.into());
    }
    use std::os::unix::process::CommandExt;
    Some(
        Command::new(program)
            .args(&arguments[2..])
            .stdin(Stdio::null())
            .exec(),
    )
}
fn failed(message: impl Into<String>) -> ExecutionError {
    ExecutionError::Failed(message.into())
}
fn io_failed(error: std::io::Error) -> ExecutionError {
    failed(format!("local tool IO: {}", error.kind()))
}
pub struct LocalTools {
    root: PathBuf,
    root_directory: File,
    cancelled: Arc<AtomicBool>,
    events: Sender,
}
struct Target {
    parent: File,
    name: OsString,
    relative: PathBuf,
}
pub fn definitions(actor: &Actor, permission: Permission) -> Vec<Tool> {
    let string = || json!({"type":"string","minLength":1,"maxLength":65536});
    let specs = [
        ("list_directory", "List a directory in this workspace. Output is bounded and indicates truncation.", json!({"path":string()}), vec!["path"], Effect::Read),
        ("read_file", "Read a UTF-8 workspace file up to 64 KiB; returns its digest for safe replacement.", json!({"path":string()}), vec!["path"], Effect::Read),
        ("search", "Search workspace file contents with ripgrep. Ignores ignored files; no symlink following. Use list_directory to explore.", json!({"pattern":string()}), vec!["pattern"], Effect::Read),
        ("write_file", "Create a UTF-8 file or replace it. Replacing requires expected_digest from read_file; parent directory must exist.", json!({"path":string(),"content":{"type":"string","maxLength":65536},"expected_digest":string()}), vec!["path","content"], Effect::Write),
        ("edit_file", "Replace one unique exact old_text occurrence in a workspace UTF-8 file. Read the file first; unrelated content is preserved.", json!({"path":string(),"old_text":string(),"new_text":{"type":"string","maxLength":65536}}), vec!["path","old_text","new_text"], Effect::Write),
        ("shell", "Execute a POSIX shell command with workspace cwd and the user's OS permissions. No filesystem sandbox. Timeout 1..120 seconds; output is bounded. Background jobs are not retained. Nonzero exit is evidence of failure, not success.", json!({"command":string(),"timeout_seconds":{"type":"integer","minimum":1,"maximum":120}}), vec!["command"], Effect::Write),
    ];
    specs.into_iter().filter(|spec| permission != Permission::ReadOnly || spec.4 == Effect::Read).map(|(name, description, properties, required, effect)| Tool {
        id:name.into(), workspace_id:actor.workspace_id.clone(), version:1, schema_version:SCHEMA_VERSION, name:name.into(), description:description.into(),
        input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}), output_schema:None,
        execution:Execution::Registered { key:name.into() }, credential_ref:None,
        policy_ref:if effect == Effect::Read {"local-read"} else {"local-write"}.into(), effect, created_at:0,
    }).collect()
}
impl LocalTools {
    pub fn new(
        root: PathBuf,
        events: Sender,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, ExecutionError> {
        if !root.is_absolute() || root.canonicalize().map_err(io_failed)? != root {
            return Err(failed("workspace must be a canonical absolute directory"));
        }
        let base: File = rustix::fs::open(
            "/",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| io_failed(error.into()))?
        .into();
        let root_directory = Self::walk(
            &base,
            root.strip_prefix("/")
                .map_err(|_| failed("absolute workspace required"))?,
        )?;
        Ok(Self {
            root,
            root_directory,
            events,
            cancelled,
        })
    }

    fn relative(&self, name: &str) -> Result<PathBuf, ExecutionError> {
        let input = Path::new(name);
        let relative = if input.is_absolute() {
            input
                .strip_prefix(&self.root)
                .map_err(|_| failed("path is outside the session workspace"))?
        } else {
            input
        };
        let mut result = PathBuf::new();
        for component in relative.components() {
            match component {
                Component::Normal(name) => result.push(name),
                Component::CurDir => (),
                _ => return Err(failed("workspace paths must not contain parent traversal")),
            }
        }
        Ok(result)
    }

    fn walk(base: &File, relative: &Path) -> Result<File, ExecutionError> {
        let mut directory = base.try_clone().map_err(io_failed)?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(failed("invalid directory component"));
            };
            directory = rustix::fs::openat(
                &directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|error| io_failed(error.into()))?
            .into();
        }
        Ok(directory)
    }

    fn target(&self, name: &str) -> Result<Target, ExecutionError> {
        let relative = self.relative(name)?;
        let name = relative
            .file_name()
            .ok_or_else(|| failed("file name required"))?
            .to_owned();
        let parent = Self::walk(
            &self.root_directory,
            relative
                .parent()
                .ok_or_else(|| failed("file parent required"))?,
        )?;
        Ok(Target {
            parent,
            name,
            relative,
        })
    }

    fn open_read(target: &Target) -> Result<Option<File>, ExecutionError> {
        let file: File = match rustix::fs::openat(
            &target.parent,
            &target.name,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        ) {
            Ok(file) => file.into(),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(io_failed(error.into())),
        };
        if !file.metadata().map_err(io_failed)?.is_file() {
            return Err(failed("path must be a regular file"));
        }
        Ok(Some(file))
    }

    fn read(target: &Target) -> Result<String, ExecutionError> {
        let file = Self::open_read(target)?.ok_or_else(|| failed("file not found"))?;
        Self::read_open(file)
    }

    fn read_open(file: File) -> Result<String, ExecutionError> {
        Self::read_bounded(
            file,
            FILE_LIMIT,
            "file exceeds 64 KiB; use a bounded shell command with approval",
        )
    }

    fn read_bounded(file: File, limit: usize, too_large: &str) -> Result<String, ExecutionError> {
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_failed)?;
        if bytes.len() > limit {
            return Err(failed(too_large));
        }
        String::from_utf8(bytes).map_err(|_| failed("file is not UTF-8"))
    }

    pub fn project_instructions(&self) -> Result<Option<String>, ExecutionError> {
        Self::open_read(&self.target("AGENTS.md")?)?
            .map(|file| Self::read_bounded(file, 32 * 1024, "AGENTS.md exceeds 32 KiB"))
            .transpose()
    }

    fn search_program(&self, paths: Option<&OsStr>) -> Result<PathBuf, ExecutionError> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = self.root_directory.metadata().map_err(io_failed)?;
        if let Some(paths) = paths {
            for directory in std::env::split_paths(paths).filter(|path| path.is_absolute()) {
                let Ok(program) = directory.join("rg").canonicalize() else {
                    continue;
                };
                // Search is a read tool: a checkout-provided executable must
                // never turn a read-only search into unapproved code execution.
                let outside = program
                    .ancestors()
                    .skip(1)
                    .try_fold(true, |outside, parent| {
                        let metadata = std::fs::metadata(parent).map_err(io_failed)?;
                        Ok::<_, ExecutionError>(
                            outside
                                && (metadata.dev() != root.dev() || metadata.ino() != root.ino()),
                        )
                    })?;
                if !outside {
                    continue;
                }
                if std::fs::metadata(&program).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                }) {
                    return Ok(program);
                }
            }
        }
        Err(failed("search requires a trusted ripgrep executable; install ripgrep/rg on PATH outside the workspace"))
    }
    fn write(
        &self,
        target: &Target,
        old: Option<&str>,
        content: &str,
    ) -> Result<Value, ExecutionError> {
        if content.len() > FILE_LIMIT {
            return Err(failed("resulting file exceeds 64 KiB"));
        }
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(failed("cancelled before file write"));
        }
        let temporary = format!(".hudson-{}.tmp", uuid::Uuid::new_v4());
        let mut file: File = rustix::fs::openat(
            &target.parent,
            temporary.as_str(),
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0o600),
        )
        .map_err(|error| io_failed(error.into()))?
        .into();
        let result = (|| {
            file.write_all(content.as_bytes()).map_err(io_failed)?;
            if old.is_some() {
                let existing = Self::open_read(target)?
                    .ok_or_else(|| failed("file changed during edit; read it again"))?;
                file.set_permissions(existing.metadata().map_err(io_failed)?.permissions())
                    .map_err(io_failed)?;
            }
            file.sync_all().map_err(io_failed)?;
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(failed("cancelled before file publication"));
            }
            if let Some(expected) = old {
                if Self::read(target)? != expected {
                    return Err(failed("file changed during edit; read it again"));
                }
                rustix::fs::renameat(
                    &target.parent,
                    temporary.as_str(),
                    &target.parent,
                    &target.name,
                )
                .map_err(|_| {
                    ExecutionError::Unknown("file replacement outcome requires inspection".into())
                })?;
            } else {
                // Hard linking is an atomic create-if-absent; never clobber a file
                // concurrently created after the initial existence check.
                rustix::fs::linkat(
                    &target.parent,
                    temporary.as_str(),
                    &target.parent,
                    &target.name,
                    rustix::fs::AtFlags::empty(),
                )
                .map_err(|error| io_failed(error.into()))?;
            }
            Ok(())
        })();
        let _ = rustix::fs::unlinkat(
            &target.parent,
            temporary.as_str(),
            rustix::fs::AtFlags::empty(),
        );
        result?;
        target.parent.sync_all().map_err(|_| {
            ExecutionError::Unknown(
                "file changed but directory durability is uncertain; inspect effects".into(),
            )
        })?;
        let before = old.unwrap_or("");
        let diff = format!(
            "{}\n{}\n{}",
            target.relative.display(),
            before
                .lines()
                .take(60)
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            content
                .lines()
                .take(60)
                .map(|s| format!("+ {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let _ = self.events.try_send(Event::Activity(diff));
        Ok(
            json!({"written":true,"bytes":content.len(),"digest":hudson_core::definitions::digest(&content).map_err(|_|failed("digest failed"))?}),
        )
    }
    fn execute_tool(&self, name: &str, args: &Value) -> Result<Value, ExecutionError> {
        let field = |key: &str| {
            args[key]
                .as_str()
                .ok_or_else(|| failed(format!("{key} must be text")))
        };
        match name {
            "list_directory" => {
                let directory = Self::walk(&self.root_directory, &self.relative(field("path")?)?)?;
                let iterator = rustix::fs::Dir::read_from(&directory)
                    .map_err(|error| io_failed(error.into()))?;
                let mut entries = Vec::new();
                let mut truncated = false;
                for entry in iterator {
                    let entry = entry.map_err(|error| io_failed(error.into()))?;
                    let name = entry.file_name();
                    if matches!(name.to_bytes(), b"." | b"..") {
                        continue;
                    }
                    if entries.len() == 200 {
                        truncated = true;
                        break;
                    }
                    let kind = if entry.file_type() == rustix::fs::FileType::Unknown {
                        let metadata = rustix::fs::statat(
                            &directory,
                            name,
                            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
                        )
                        .map_err(|error| io_failed(error.into()))?;
                        rustix::fs::FileType::from_raw_mode(metadata.st_mode)
                    } else {
                        entry.file_type()
                    };
                    entries.push(json!({"name":name.to_string_lossy(),"directory":kind == rustix::fs::FileType::Directory}));
                }
                entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                Ok(json!({"entries":entries,"truncated":truncated}))
            }
            "read_file" => {
                let content = Self::read(&self.target(field("path")?)?)?;
                Ok(
                    json!({"digest":hudson_core::definitions::digest(&content).map_err(|_|failed("digest failed"))?,"content":content}),
                )
            }
            "write_file" => {
                let target = self.target(field("path")?)?;
                let old = Self::open_read(&target)?.map(Self::read_open).transpose()?;
                if let Some(old) = &old {
                    if args["expected_digest"].as_str()
                        != Some(
                            hudson_core::definitions::digest(old)
                                .map_err(|_| failed("digest failed"))?
                                .as_str(),
                        )
                    {
                        return Err(failed(
                            "expected_digest does not match; read the file again",
                        ));
                    }
                }
                self.write(&target, old.as_deref(), field("content")?)
            }
            "edit_file" => {
                let target = self.target(field("path")?)?;
                let old = Self::read(&target)?;
                let needle = field("old_text")?;
                if needle.is_empty() || old.matches(needle).count() != 1 {
                    return Err(failed("old_text must match exactly once"));
                }
                let content = old.replacen(needle, field("new_text")?, 1);
                self.write(&target, Some(&old), &content)
            }
            "search" => {
                let mut command =
                    Command::new(self.search_program(std::env::var_os("PATH").as_deref())?);
                command.args([
                    "--line-number",
                    "--no-heading",
                    "--color",
                    "never",
                    "--max-count",
                    "50",
                    "--",
                    field("pattern")?,
                    ".",
                ]);
                self.command(command, Duration::from_secs(15))
            }
            "shell" => {
                #[cfg(not(unix))]
                {
                    return Err(failed("local shell execution currently requires Unix"));
                }
                #[cfg(unix)]
                {
                    let mut command = Command::new("/bin/sh");
                    command.args(["-c", field("command")?]);
                    self.command(
                        command,
                        Duration::from_secs(
                            args["timeout_seconds"]
                                .as_u64()
                                .unwrap_or(120)
                                .clamp(1, 120),
                        ),
                    )
                }
            }
            _ => Err(failed("unknown local tool")),
        }
    }
    #[cfg(not(unix))]
    fn command(&self, _command: Command, _timeout: Duration) -> Result<Value, ExecutionError> {
        Err(failed("local process tools currently require Unix"))
    }
    #[cfg(unix)]
    fn command(&self, command: Command, timeout: Duration) -> Result<Value, ExecutionError> {
        let mut command = self.process_in_workspace(command)?;
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        for key in ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut process = RunningChild {
            child: command.spawn().map_err(io_failed)?,
            stopped: false,
        };
        let mut stdout = process
            .child
            .stdout
            .take()
            .ok_or_else(|| failed("stdout pipe unavailable"))?;
        let mut stderr = process
            .child
            .stderr
            .take()
            .ok_or_else(|| failed("stderr pipe unavailable"))?;
        for fd in [
            &stdout as &dyn std::os::fd::AsFd,
            &stderr as &dyn std::os::fd::AsFd,
        ] {
            let flags = rustix::fs::fcntl_getfl(fd).map_err(|_| {
                ExecutionError::Unknown("command pipe setup failed; inspect effects".into())
            })?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK).map_err(|_| {
                ExecutionError::Unknown("command pipe setup failed; inspect effects".into())
            })?;
        }
        let start = Instant::now();
        let mut output = Vec::new();
        let mut truncated = false;
        let mut interrupted = false;
        let mut drain = |pipe: &mut dyn Read, chunks: usize| -> Result<(), ExecutionError> {
            let mut bytes = [0; 2048];
            for _ in 0..chunks {
                match pipe.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        let remaining = OUTPUT_LIMIT.saturating_sub(output.len());
                        let retained = count.min(remaining);
                        output.extend_from_slice(&bytes[..retained]);
                        truncated |= count > remaining;
                        if retained > 0 {
                            let _ = self.events.try_send(Event::Activity(
                                String::from_utf8_lossy(&bytes[..retained]).into_owned(),
                            ));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        return Err(ExecutionError::Unknown(
                            "command output observation failed; inspect effects".into(),
                        ))
                    }
                }
            }
            Ok(())
        };
        let status = loop {
            // Bounded reads prevent a noisy child from starving cancellation.
            drain(&mut stdout, 8)?;
            drain(&mut stderr, 8)?;
            if self.cancelled.load(Ordering::Relaxed) || start.elapsed() >= timeout {
                interrupted = true;
                process.stop();
                break process.child.wait().map_err(|_| {
                    ExecutionError::Unknown("command wait failed; inspect effects".into())
                })?;
            }
            if process.exited()? {
                process.stop();
                break process.child.wait().map_err(|_| {
                    ExecutionError::Unknown("command wait failed; inspect effects".into())
                })?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        drain(&mut stdout, 64)?;
        drain(&mut stderr, 64)?;
        if interrupted {
            return Err(ExecutionError::Unknown(
                "command cancelled or timed out; partial effects may exist and require inspection"
                    .into(),
            ));
        }
        Ok(
            json!({"exit_code":status.code(),"success":status.success(),"output":String::from_utf8_lossy(&output),"truncated":truncated}),
        )
    }
    fn process_in_workspace(&self, target: Command) -> Result<Command, ExecutionError> {
        let executable = std::env::current_exe().map_err(io_failed)?;
        #[cfg(test)]
        let executable = executable.parent().and_then(|parent| parent.parent())
            .ok_or_else(|| failed("build Hudson binaries with `cargo build --locked --bins` before local process tests"))?
            .join("hudson");
        if !executable.is_file() {
            return Err(failed("build Hudson binaries with `cargo build --locked --bins` before local process tests"));
        }
        let mut command = Command::new(executable);
        command
            .arg(INTERNAL_EXEC)
            .arg(target.get_program())
            .args(target.get_args());
        // The fresh helper receives one owned directory capability at fd 0.
        // It changes only its own cwd, then replaces stdin before target exec.
        command.stdin(Stdio::from(
            self.root_directory.try_clone().map_err(io_failed)?,
        ));
        Ok(command)
    }
}
struct RunningChild {
    child: std::process::Child,
    stopped: bool,
}
impl RunningChild {
    fn exited(&mut self) -> Result<bool, ExecutionError> {
        let pid = rustix::process::Pid::from_raw(self.child.id() as i32).ok_or_else(|| {
            ExecutionError::Unknown("command PID unavailable; inspect effects".into())
        })?;
        loop {
            match rustix::process::waitid(
                rustix::process::WaitId::Pid(pid),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            ) {
                Ok(status) => return Ok(status.is_some()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => {
                    if error == rustix::io::Errno::CHILD {
                        // An external reaper invalidated our PID ownership.
                        // Never signal a potentially reused process group.
                        self.stopped = true;
                    }
                    return Err(ExecutionError::Unknown(
                        "command observation failed; inspect effects".into(),
                    ));
                }
            }
        }
    }

    fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            // Every wait/reap happens after this cleanup. NOWAIT observation
            // keeps the exited leader's PID reserved until its group is killed.
            kill_group(&mut self.child);
        }
    }
}
impl Drop for RunningChild {
    fn drop(&mut self) {
        self.stop();
        let _ = self.child.wait();
    }
}
#[cfg(unix)]
fn kill_group(child: &mut std::process::Child) {
    if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
}
#[cfg(not(unix))]
fn kill_group(child: &mut std::process::Child) {
    let _ = child.kill();
}
impl ToolExecutor for LocalTools {
    fn execute(&mut self, invocation: Invocation<'_>) -> Result<Value, ExecutionError> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(failed("cancelled before tool dispatch"));
        }
        let Execution::Registered { key } = &invocation.tool.execution else {
            return Err(failed("local executor requires registered tools"));
        };
        let _ = self
            .events
            .try_send(Event::Activity(format!("{} {}", key, invocation.arguments)));
        self.execute_tool(key, invocation.arguments)
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
