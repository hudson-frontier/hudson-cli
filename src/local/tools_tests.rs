use super::*;
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    sync::mpsc,
};

fn tools(root: &Path) -> LocalTools {
    LocalTools::new(
        root.canonicalize().unwrap(),
        mpsc::sync_channel(512).0,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}

#[test]
fn parent_swap_cannot_redirect_read_replace_or_create() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(root.path().join("nested/file"), "inside").unwrap();
    fs::write(outside.path().join("file"), "outside").unwrap();
    let tools = tools(root.path());
    let existing = tools.target("nested/file").unwrap();
    let new = tools.target("nested/new").unwrap();
    fs::rename(root.path().join("nested"), root.path().join("original")).unwrap();
    symlink(outside.path(), root.path().join("nested")).unwrap();

    assert_eq!(LocalTools::read(&existing).unwrap(), "inside");
    tools.write(&existing, Some("inside"), "updated").unwrap();
    tools.write(&new, None, "created").unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("original/file")).unwrap(),
        "updated"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("original/new")).unwrap(),
        "created"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("file")).unwrap(),
        "outside"
    );
    assert!(!outside.path().join("new").exists());
    for name in ["nested/file", "nested/new"] {
        assert!(tools.target(name).is_err());
    }
    assert!(tools
        .execute_tool("list_directory", &json!({"path":"nested"}))
        .is_err());
}

#[test]
fn filesystem_tools_reject_traversal_and_all_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(root.path().join("file"), "inside").unwrap();
    fs::write(outside.path().join("secret"), "outside").unwrap();
    symlink(outside.path(), root.path().join("escape")).unwrap();
    symlink("file", root.path().join("internal-link")).unwrap();
    symlink(
        outside.path().join("secret"),
        root.path().join("external-link"),
    )
    .unwrap();
    let tools = tools(root.path());
    assert!(tools.target("../secret").is_err());
    assert!(tools
        .target(outside.path().join("secret").to_str().unwrap())
        .is_err());
    for name in ["escape/secret", "internal-link", "external-link"] {
        assert!(tools
            .execute_tool("read_file", &json!({"path":name}))
            .is_err());
        assert!(tools
            .execute_tool("write_file", &json!({"path":name,"content":"no"}))
            .is_err());
    }
    assert_eq!(
        fs::read_to_string(root.path().join("file")).unwrap(),
        "inside"
    );
    assert_eq!(
        fs::read_to_string(outside.path().join("secret")).unwrap(),
        "outside"
    );
}

#[test]
fn bounded_regular_utf8_files_and_directory_listing() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("binary"), [0xff, 0xfe]).unwrap();
    fs::write(root.path().join("large"), vec![b'x'; FILE_LIMIT + 1]).unwrap();
    fs::create_dir(root.path().join("directory")).unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(root.path().join("socket")).unwrap();
    let tools = tools(root.path());
    #[cfg(target_os = "linux")]
    {
        rustix::fs::mkfifoat(
            &tools.root_directory,
            "fifo",
            rustix::fs::Mode::from_raw_mode(0o600),
        )
        .unwrap();
        assert!(tools
            .execute_tool("read_file", &json!({"path":"fifo"}))
            .is_err());
    }
    for name in ["binary", "large", "directory", "socket"] {
        assert!(tools
            .execute_tool("read_file", &json!({"path":name}))
            .is_err());
    }
    assert!(tools
        .execute_tool(
            "write_file",
            &json!({"path":"new","content":"x".repeat(FILE_LIMIT + 1)})
        )
        .is_err());
    let listing = tools
        .execute_tool("list_directory", &json!({"path":"."}))
        .unwrap();
    assert_eq!(
        listing["entries"].as_array().unwrap().len(),
        4 + usize::from(cfg!(target_os = "linux"))
    );
    assert_eq!(listing["truncated"], false);
    assert!(listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "directory" && entry["directory"] == true));
}

#[test]
fn concurrent_create_and_stale_replace_preserve_user_files() {
    let root = tempfile::tempdir().unwrap();
    let tools = tools(root.path());
    let new = tools.target("created").unwrap();
    fs::write(root.path().join("created"), "user-created").unwrap();
    assert!(tools.write(&new, None, "clobber").is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("created")).unwrap(),
        "user-created"
    );

    let existing = tools.target("created").unwrap();
    let old = LocalTools::read(&existing).unwrap();
    fs::write(root.path().join("created"), "user-updated").unwrap();
    assert!(tools.write(&existing, Some(&old), "clobber").is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("created")).unwrap(),
        "user-updated"
    );
    fs::set_permissions(
        root.path().join("created"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    tools
        .write(&existing, Some("user-updated"), "approved")
        .unwrap();
    assert_eq!(
        fs::metadata(root.path().join("created"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert!(fs::read_dir(root.path()).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".hudson-")));
}

#[test]
fn renamed_workspace_stays_pinned_for_files_and_shell_cwd() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("marker"), "inside").unwrap();
    fs::write(outside.path().join("marker"), "outside").unwrap();
    let tools = tools(&original);
    fs::rename(&original, &moved).unwrap();
    symlink(outside.path(), &original).unwrap();
    let read = tools
        .execute_tool("read_file", &json!({"path":"marker"}))
        .unwrap();
    assert_eq!(read["content"], "inside");
    let result = tools
        .execute_tool("shell", &json!({"command":"cat marker"}))
        .unwrap();
    assert_eq!(result["success"], true);
    assert_eq!(result["output"], "inside");
}

#[test]
fn timeout_kills_the_process_group_with_unknown_effects() {
    let root = tempfile::tempdir().unwrap();
    let tools = tools(root.path());
    let result = tools.execute_tool("shell", &json!({"command":"(sleep 2; printf orphan > orphan) & printf started > started; wait","timeout_seconds":1}));
    assert!(matches!(result, Err(ExecutionError::Unknown(_))));
    assert!(root.path().join("started").is_file());
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!root.path().join("orphan").exists());
}

#[test]
fn read_only_search_never_resolves_workspace_executables_or_relative_path_entries() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let program = root.path().join("rg");
    fs::write(&program, "#!/bin/sh\nprintf unsafe > marker\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&program, external.path().join("rg")).unwrap();
    let tools = tools(root.path());
    for paths in [
        None,
        Some(OsStr::new(".")),
        Some(root.path().as_os_str()),
        Some(external.path().as_os_str()),
    ] {
        let error = tools.search_program(paths).err().unwrap();
        assert!(format!("{error:?}").contains("install ripgrep/rg on PATH"));
    }
    assert!(!root.path().join("marker").exists());
}

#[test]
fn cancellation_prevents_file_publication() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("existing"), "keep").unwrap();
    let tools = tools(root.path());
    let existing = tools.target("existing").unwrap();
    let new = tools.target("new").unwrap();
    tools.cancelled.store(true, Ordering::Relaxed);
    assert!(tools.write(&existing, Some("keep"), "overwrite").is_err());
    assert!(tools.write(&new, None, "create").is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("existing")).unwrap(),
        "keep"
    );
    assert!(!root.path().join("new").exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn shell_results_keep_failure_exit_and_bounded_output() {
    let root = tempfile::tempdir().unwrap();
    let tools = tools(root.path());
    let result = tools
        .execute_tool("shell", &json!({"command":"printf '%32768s' ''; exit 7"}))
        .unwrap();
    assert_eq!(result["exit_code"], 7);
    assert_eq!(result["success"], false);
    assert_eq!(result["truncated"], true);
    assert_eq!(result["output"].as_str().unwrap().len(), OUTPUT_LIMIT);
}

#[test]
fn renamed_workspace_executables_remain_ineligible_for_read_only_search() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("rg"), "#!/bin/sh\nprintf unsafe > marker\n").unwrap();
    fs::set_permissions(root.join("rg"), fs::Permissions::from_mode(0o700)).unwrap();
    let tools = tools(&root);
    fs::rename(&root, &moved).unwrap();
    assert!(tools.search_program(Some(moved.as_os_str())).is_err());
    assert!(!moved.join("marker").exists());
}

#[test]
fn observing_exit_keeps_the_leader_waitable_until_group_cleanup() {
    use std::os::unix::process::CommandExt;
    let mut process = RunningChild {
        child: Command::new("/bin/sh")
            .args(["-c", "exit 23"])
            .process_group(0)
            .spawn()
            .unwrap(),
        stopped: false,
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !process.exited().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(process.exited().unwrap()); // NOWAIT retains the same waitable leader.
    process.stop();
    assert!(process.stopped);
    assert_eq!(process.child.wait().unwrap().code(), Some(23));
    // Drop sees stopped=true and cannot signal the reaped PID/group again.
}

#[test]
fn exited_leader_still_cleans_up_its_background_group_and_keeps_status() {
    let root = tempfile::tempdir().unwrap();
    let tools = tools(root.path());
    let result = tools
        .execute_tool(
            "shell",
            &json!({"command":"(sleep 1; printf orphan > orphan) & exit 23"}),
        )
        .unwrap();
    assert_eq!(result["exit_code"], 23);
    assert_eq!(result["success"], false);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!root.path().join("orphan").exists());
}

#[test]
fn internal_exec_requires_its_private_marker_and_target() {
    assert!(dispatch_internal(&[OsString::from("--help")]).is_none());
    assert!(dispatch_internal(&[]).is_none());
    let error = dispatch_internal(&[OsString::from(INTERNAL_EXEC)]).unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn internal_exec_replaces_directory_stdin_before_the_target_runs() {
    let root = tempfile::tempdir().unwrap();
    let tools = tools(root.path());
    let result = tools
        .execute_tool(
            "shell",
            &json!({"command":"if [ -d /dev/fd/0 ]; then exit 99; fi; printf null"}),
        )
        .unwrap();
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["output"], "null");
}

#[test]
fn project_instructions_use_the_pinned_root_and_enforce_their_own_bound() {
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(&original).unwrap();
    let tools = tools(&original);
    assert!(tools.project_instructions().unwrap().is_none());
    fs::write(original.join("AGENTS.md"), "trusted instructions").unwrap();
    fs::write(outside.path().join("AGENTS.md"), "outside instructions").unwrap();
    fs::rename(&original, &moved).unwrap();
    symlink(outside.path(), &original).unwrap();
    assert_eq!(
        tools.project_instructions().unwrap().as_deref(),
        Some("trusted instructions")
    );
    fs::write(moved.join("AGENTS.md"), vec![b'x'; 32 * 1024 + 1]).unwrap();
    assert!(tools.project_instructions().is_err());
    fs::remove_file(moved.join("AGENTS.md")).unwrap();
    symlink(outside.path().join("AGENTS.md"), moved.join("AGENTS.md")).unwrap();
    assert!(tools.project_instructions().is_err());
}

#[test]
fn constructing_tools_never_rebinds_a_canonical_workspace_through_a_symlink() {
    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = parent.path().canonicalize().unwrap().join("workspace");
    fs::create_dir(&root).unwrap();
    let selected = root.canonicalize().unwrap();
    fs::rename(&root, parent.path().join("moved")).unwrap();
    symlink(outside.path(), &root).unwrap();
    assert!(LocalTools::new(
        selected,
        mpsc::sync_channel(512).0,
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
}
