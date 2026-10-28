use super::*;
use hudson_core::adapters::tools::{ExecutionError, Invocation, ToolExecutor};
use hudson_harness::{ModelRequest, ModelResponse, ToolCall};
use std::collections::VecDeque;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hudson-local-{}", Uuid::new_v4()));
        private_directory(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Script {
    responses: VecDeque<ModelResponse>,
    follow_up: bool,
}
impl ModelExecutor for Script {
    fn call(&mut self, request: &ModelRequest) -> Result<ModelResponse, ExecutionError> {
        if self.follow_up {
            let text = serde_json::to_string(&request.messages).unwrap();
            assert!(text.contains("Fix the calculation"));
            assert!(text.contains("fixed and verified"));
            assert!(text.contains("Now explain"));
        }
        Ok(self.responses.pop_front().expect("unexpected model call"))
    }
}
fn tool(name: &str, arguments: serde_json::Value) -> ModelResponse {
    ModelResponse::ToolCalls {
        calls: vec![ToolCall {
            call_id: Uuid::new_v4().to_string(),
            name: name.into(),
            arguments,
            provider_metadata: serde_json::Value::Null,
        }],
    }
}
fn build_host(
    session: Session,
    directory: SessionDirectory,
    store: Store,
    model: Box<dyn ModelExecutor>,
) -> Result<Host, Error> {
    let (events, _) = mpsc::sync_channel(512);
    let cancelled = Arc::new(AtomicBool::new(false));
    let tools =
        tools::LocalTools::new(session.workspace.clone(), events.clone(), cancelled.clone())
            .unwrap();
    Host::build(session, directory, store, model, events, cancelled, tools)
}
fn host(
    root: &Path,
    directory: &Path,
    permission: Permission,
    responses: Vec<ModelResponse>,
) -> Host {
    fs::create_dir_all(directory).unwrap();
    let session = Session {
        schema_version: 1,
        id: Uuid::new_v4(),
        workspace: root.into(),
        permission,
        model: "fixture".into(),
        endpoint: "http://127.0.0.1:1".into(),
        instructions: "Complete the task using tools.".into(),
        turns: vec![],
    };
    let directory = SessionDirectory::open(directory).unwrap();
    let store = Store::local_file_in(directory.file(), "state.json").unwrap();
    build_host(
        session,
        directory,
        store,
        Box::new(Script {
            responses: responses.into(),
            follow_up: false,
        }),
    )
    .unwrap()
}

#[test]
fn resume_locks_before_reading_metadata_and_cannot_overwrite_a_newer_turn() {
    use std::time::Duration;
    const TIMEOUT: Duration = Duration::from_secs(20);
    enum Attempt {
        MetadataRead,
        Finished(Result<usize, String>),
    }
    let root = Directory::new();
    let state = Directory::new();
    let mut first = host(
        &root.0,
        &state.0,
        Permission::ReadOnly,
        vec![
            ModelResponse::Final {
                output: json!("first answer"),
            },
            ModelResponse::Final {
                output: json!("second answer"),
            },
        ],
    );
    first.prompt("First task".into()).unwrap();
    first.drive(&mpsc::channel().1).unwrap();
    let first_id = first.active().unwrap();
    let (observed, observations) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let state_path = state.0.clone();
    let second = std::thread::spawn(move || {
        let read = observed.clone();
        SESSION_READ_OBSERVER.with(|observer| {
            observer.replace(Some(Box::new(move || {
                read.send(Attempt::MetadataRead).unwrap();
                released.recv_timeout(TIMEOUT).unwrap();
            })));
        });
        let directory = SessionDirectory::open(&state_path).unwrap();
        let opened = open_session_state(&directory, true);
        SESSION_READ_OBSERVER.with(|observer| observer.take());
        let result = opened
            .and_then(|(store, saved)| {
                build_host(
                    saved.expect("resume metadata"),
                    directory,
                    store,
                    Box::new(Script {
                        responses: VecDeque::new(),
                        follow_up: false,
                    }),
                )
                .map(|host| host.session.turns.len())
            })
            .map_err(|error| error.to_string());
        observed.send(Attempt::Finished(result)).unwrap();
    });
    let observed = observations.recv_timeout(TIMEOUT).unwrap();
    let read_before_lock = matches!(&observed, Attempt::MetadataRead);
    first.prompt("Second task".into()).unwrap();
    first.drive(&mpsc::channel().1).unwrap();
    let expected = [first_id, first.active().unwrap()];
    assert_eq!(read_session(&first.directory).unwrap().turns.len(), 2);
    drop(first);
    let result = match observed {
        Attempt::MetadataRead => {
            // The legacy ordering paused here with turn 1 already loaded.
            // Let it acquire the now-released lock and exercise Host::build.
            release.send(()).unwrap();
            match observations.recv_timeout(TIMEOUT).unwrap() {
                Attempt::Finished(result) => result,
                Attempt::MetadataRead => panic!("metadata read more than once"),
            }
        }
        Attempt::Finished(result) => result,
    };
    second.join().unwrap();
    let directory = SessionDirectory::open(&state.0).unwrap();
    let recorded = read_session(&directory).unwrap();
    assert!(
        !read_before_lock && result.is_err(),
        "a concurrent resume read stale metadata and retained only {} turns",
        recorded.turns.len()
    );
    let (store, saved) = open_session_state(&directory, true).unwrap();
    let reopened = build_host(
        saved.unwrap(),
        directory,
        store,
        Box::new(Script {
            responses: VecDeque::new(),
            follow_up: false,
        }),
    )
    .unwrap();
    assert_eq!(
        reopened
            .session
            .turns
            .iter()
            .map(|turn| turn.run)
            .collect::<Vec<_>>(),
        expected
    );
    for id in expected {
        let view = reopened.runtime.store.inspect(&reopened.actor, id).unwrap();
        assert_eq!(view.status, RunStatus::Completed);
        assert_eq!(view.usage.model_calls, 1);
    }
}

#[test]
fn resume_without_a_core_snapshot_does_not_bootstrap_new_state() {
    let root = Directory::new();
    let state = Directory::new();
    let first = host(&root.0, &state.0, Permission::ReadOnly, vec![]);
    let metadata = fs::read(state.0.join("session.json")).unwrap();
    drop(first);
    fs::remove_file(state.0.join("state.json")).unwrap();
    let directory = SessionDirectory::open(&state.0).unwrap();
    assert!(open_session_state(&directory, true).is_err());
    assert!(!state.0.join("state.json").exists());
    assert_eq!(fs::read(state.0.join("session.json")).unwrap(), metadata);
}

#[test]
fn local_tools_fix_a_fixture_and_follow_up_survives_reopen() {
    let root = Directory::new();
    let state = Directory::new();
    fs::write(
        root.0.join("calculation.py"),
        "value = 1\nassert value == 2\n",
    )
    .unwrap();
    let responses = vec![
        tool("read_file", json!({"path":"calculation.py"})),
        tool("shell", json!({"command":"python3 calculation.py"})),
        tool(
            "edit_file",
            json!({"path":"calculation.py","old_text":"value = 1","new_text":"value = 2"}),
        ),
        tool("shell", json!({"command":"python3 calculation.py"})),
        ModelResponse::Final {
            output: json!("fixed and verified"),
        },
    ];
    let mut host = host(&root.0, &state.0, Permission::FullAccess, responses);
    host.prompt("Fix the calculation".into()).unwrap();
    host.drive(&mpsc::channel().1).unwrap();
    let id = host.active().unwrap();
    let view = host.runtime.store.inspect(&host.actor, id).unwrap();
    assert_eq!(view.status, RunStatus::Completed);
    assert_eq!(view.usage.tool_calls, 4);
    let operations = host.runtime.store.operations(&host.actor, id).unwrap();
    let results: Vec<_> = operations
        .iter()
        .filter_map(|op| match &op.result {
            Some(OperationResult::Tool { result }) => {
                Some(serde_json::to_string(&result.outcome).unwrap())
            }
            _ => None,
        })
        .collect();
    assert!(results.iter().any(|r| r.contains("\"success\":false")));
    assert!(results.iter().any(|r| r.contains("\"success\":true")));
    assert!(fs::read_to_string(root.0.join("calculation.py"))
        .unwrap()
        .contains("value = 2"));
    let session: Session =
        serde_json::from_slice(&fs::read(state.0.join("session.json")).unwrap()).unwrap();
    drop(host);
    let directory = SessionDirectory::open(&state.0).unwrap();
    let store = Store::local_file_in(directory.file(), "state.json").unwrap();
    let mut reopened = build_host(
        session,
        directory,
        store,
        Box::new(Script {
            responses: vec![ModelResponse::Final {
                output: json!("Explanation"),
            }]
            .into(),
            follow_up: true,
        }),
    )
    .unwrap();
    reopened.prompt("Now explain".into()).unwrap();
    reopened.drive(&mpsc::channel().1).unwrap();
    assert_eq!(
        reopened
            .runtime
            .store
            .inspect(&reopened.actor, reopened.active().unwrap())
            .unwrap()
            .status,
        RunStatus::Completed
    );
    assert_eq!(
        reopened
            .runtime
            .store
            .inspect(&reopened.actor, id)
            .unwrap()
            .usage
            .tool_calls,
        4
    );
    let foreign = Actor {
        workspace_id: reopened.actor.workspace_id.clone(),
        id: "someone-else".into(),
    };
    assert!(reopened
        .runtime
        .submit_turn(
            &foreign,
            reopened.reference.clone(),
            json!("steal context"),
            None,
            id
        )
        .is_err());
    let key = Some("repeat-turn".into());
    let first = reopened
        .runtime
        .submit_turn(
            &reopened.actor,
            reopened.reference.clone(),
            json!("continue"),
            key.clone(),
            id,
        )
        .unwrap();
    assert_eq!(
        first,
        reopened
            .runtime
            .submit_turn(
                &reopened.actor,
                reopened.reference.clone(),
                json!("continue"),
                key,
                id
            )
            .unwrap()
    );
}

#[test]
#[cfg(unix)]
fn an_uncertain_shell_write_is_not_replayed_after_reopen() {
    let root = Directory::new();
    let state = Directory::new();
    let mut host = host(
        &root.0,
        &state.0,
        Permission::FullAccess,
        vec![tool(
            "shell",
            json!({"command":"printf x >> count; sleep 3","timeout_seconds":1}),
        )],
    );
    host.prompt("Do bounded work".into()).unwrap();
    host.drive(&mpsc::channel().1).unwrap();
    assert_eq!(
        host.runtime
            .store
            .inspect(&host.actor, host.active().unwrap())
            .unwrap()
            .status,
        RunStatus::Waiting
    );
    let session: Session =
        serde_json::from_slice(&fs::read(state.0.join("session.json")).unwrap()).unwrap();
    drop(host);
    let directory = SessionDirectory::open(&state.0).unwrap();
    let store = Store::local_file_in(directory.file(), "state.json").unwrap();
    let mut reopened = build_host(
        session,
        directory,
        store,
        Box::new(Script {
            responses: VecDeque::new(),
            follow_up: false,
        }),
    )
    .unwrap();
    reopened.drive(&mpsc::channel().1).unwrap();
    assert_eq!(fs::read_to_string(root.0.join("count")).unwrap(), "x");
}
#[test]
fn approval_precedes_actual_file_mutation() {
    let root = Directory::new();
    let state = Directory::new();
    let mut host = host(
        &root.0,
        &state.0,
        Permission::Ask,
        vec![
            tool("write_file", json!({"path":"new.txt","content":"approved"})),
            ModelResponse::Final {
                output: json!("done"),
            },
        ],
    );
    host.prompt("Create a file".into()).unwrap();
    host.drive(&mpsc::channel().1).unwrap();
    assert!(!root.0.join("new.txt").exists());
    let id = host.active().unwrap();
    let view = host.runtime.store.inspect(&host.actor, id).unwrap();
    let Some(WaitReason::Approval { operation_id }) = view.wait else {
        panic!("approval required")
    };
    host.runtime
        .approve(&host.actor, operation_id, true, now(), now() + 60_000)
        .unwrap();
    host.drive(&mpsc::channel().1).unwrap();
    assert_eq!(
        fs::read_to_string(root.0.join("new.txt")).unwrap(),
        "approved"
    );
}
#[test]
fn file_scope_conflicts_and_uncertain_shell_effects_are_preserved() {
    let root = Directory::new();
    let outside = Directory::new();
    let (events, _receiver) = mpsc::sync_channel(512);
    let mut tools =
        tools::LocalTools::new(root.0.clone(), events, Arc::new(AtomicBool::new(false))).unwrap();
    let actor = Actor {
        workspace_id: "test".into(),
        id: "test".into(),
    };
    let definitions = tools::definitions(&actor, Permission::FullAccess);
    let mut call = |name: &str, arguments: serde_json::Value| {
        tools.execute(Invocation {
            operation_id: Uuid::new_v4(),
            tool: definitions.iter().find(|t| t.name == name).unwrap(),
            arguments: &arguments,
        })
    };
    assert!(call("read_file", json!({"path":outside.0.join("secret")})).is_err());
    fs::write(root.0.join("a"), "user edits").unwrap();
    assert!(call(
        "write_file",
        json!({"path":"a","content":"clobber","expected_digest":"stale"})
    )
    .is_err());
    assert_eq!(fs::read_to_string(root.0.join("a")).unwrap(), "user edits");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside.0, root.0.join("escape")).unwrap();
        assert!(call("write_file", json!({"path":"escape/new","content":"no"})).is_err());
        let value = call(
            "shell",
            json!({"command":"printf done > partial; sleep 5","timeout_seconds":1}),
        );
        assert!(matches!(value, Err(ExecutionError::Unknown(_))));
        assert_eq!(fs::read_to_string(root.0.join("partial")).unwrap(), "done");
    }
    let readonly = tools::definitions(&actor, Permission::ReadOnly);
    assert!(readonly.iter().all(|t| t.effect == Effect::Read));
}

#[test]
fn shutdown_drops_queued_tasks_without_admitting_or_dispatching_them() {
    let root = Directory::new();
    let state = Directory::new();
    let host = host(
        &root.0,
        &state.0,
        Permission::FullAccess,
        vec![tool(
            "write_file",
            json!({"path":"must-not-exist","content":"no"}),
        )],
    );
    let shutdown = host.shutdown.clone();
    let cancelled = host.cancelled.clone();
    let (sender, commands) = mpsc::sync_channel(32);
    sender.send(Command::Prompt("queued task".into())).unwrap();
    sender.send(Command::Resume).unwrap();
    drop(sender);
    shutdown.store(true, Ordering::Release);
    cancelled.store(true, Ordering::Relaxed);
    let directory = SessionDirectory::open(&state.0).unwrap();
    host.work(commands);
    assert!(cancelled.load(Ordering::Relaxed));
    assert!(read_session(&directory).unwrap().turns.is_empty());
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn session_save_failure_blocks_resume_and_all_later_dispatch() {
    let root = Directory::new();
    let state = Directory::new();
    let mut host = host(
        &root.0,
        &state.0,
        Permission::FullAccess,
        vec![tool(
            "write_file",
            json!({"path":"must-not-exist","content":"no"}),
        )],
    );
    fs::remove_file(state.0.join("session.json")).unwrap();
    fs::create_dir(state.0.join("session.json")).unwrap();
    assert!(host.prompt("save fails before execution".into()).is_err());
    assert!(host.poisoned);
    let active = host.active().unwrap();
    assert_eq!(
        host.runtime
            .store
            .inspect(&host.actor, active)
            .unwrap()
            .status,
        RunStatus::Queued
    );
    assert!(host.drive(&mpsc::channel().1).is_err());
    assert!(host.prompt("retry must not run".into()).is_err());
    assert!(!root.0.join("must-not-exist").exists());
}

#[test]
fn metadata_limit_is_checked_before_a_durable_run_is_admitted() {
    let root = Directory::new();
    let state = Directory::new();
    let mut host = host(&root.0, &state.0, Permission::FullAccess, vec![]);
    for _ in 0..31 {
        host.session.turns.push(Turn {
            input: "x".repeat(32 * 1024),
            run: Uuid::new_v4(),
        });
    }
    let before = fs::read(state.0.join("state.json")).unwrap();
    let error = host.prompt("y".repeat(32 * 1024)).unwrap_err();
    assert!(error.to_string().contains("session metadata limit"));
    assert!(!host.poisoned);
    assert_eq!(host.session.turns.len(), 31);
    assert_eq!(fs::read(state.0.join("state.json")).unwrap(), before);
}

#[test]
fn metadata_persistence_never_follows_links_or_changes_existing_directory_permissions() {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    let root = Directory::new();
    let state = Directory::new();
    let outside = Directory::new();
    let outside_file = outside.0.join("unrelated");
    fs::write(&outside_file, "preserve me").unwrap();
    symlink(&outside.0, root.0.join("linked-directory")).unwrap();
    assert!(private_directory(&root.0.join("linked-directory")).is_err());
    fs::set_permissions(&outside.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_directory(&outside.0).is_err());
    assert_eq!(fs::metadata(&outside.0).unwrap().mode() & 0o777, 0o755);
    let host = host(&root.0, &state.0, Permission::ReadOnly, vec![]);
    symlink(&outside_file, state.0.join("session.tmp")).unwrap();
    save_session(&host.directory, &host.session).unwrap();
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), "preserve me");
    assert_eq!(
        fs::metadata(state.0.join("session.json")).unwrap().mode() & 0o777,
        0o600
    );
    fs::remove_file(state.0.join("session.json")).unwrap();
    symlink(&outside_file, state.0.join("session.json")).unwrap();
    assert!(read_session(&host.directory).is_err());
    assert!(save_session(&host.directory, &host.session).is_err());
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), "preserve me");
}

#[test]
fn metadata_reads_reject_public_files_hard_links_and_fifos_without_blocking() {
    use std::os::unix::fs::PermissionsExt;
    let root = Directory::new();
    let state = Directory::new();
    let host = host(&root.0, &state.0, Permission::ReadOnly, vec![]);
    let path = state.0.join("session.json");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_session(&host.directory).is_err());
    assert!(save_session(&host.directory, &host.session).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&path, state.0.join("linked-metadata")).unwrap();
    assert!(read_session(&host.directory).is_err());
    fs::remove_file(&path).unwrap();
    assert!(std::process::Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let started = std::time::Instant::now();
    assert!(read_session(&host.directory).is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

#[test]
fn session_metadata_and_core_state_share_the_pinned_directory_after_a_parent_swap() {
    use std::os::unix::fs::symlink;
    let root = Directory::new();
    let parent = Directory::new();
    let state = parent.0.join("session");
    private_directory(&state).unwrap();
    let outside = Directory::new();
    fs::write(outside.0.join("session.json"), "outside metadata").unwrap();
    fs::write(outside.0.join("state.json"), "outside snapshot").unwrap();
    let mut host = host(
        &root.0,
        &state,
        Permission::ReadOnly,
        vec![ModelResponse::Final {
            output: json!("completed in the original directory"),
        }],
    );
    let retained = parent.0.join("retained");
    fs::rename(&state, &retained).unwrap();
    symlink(&outside.0, &state).unwrap();
    host.prompt("stay in the original session directory".into())
        .unwrap();
    host.drive(&mpsc::channel().1).unwrap();
    let session = read_session(&host.directory).unwrap();
    assert_eq!(session.turns.len(), 1);
    let run = host
        .runtime
        .store
        .inspect(&host.actor, session.turns[0].run)
        .unwrap();
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(
        fs::read_to_string(outside.0.join("session.json")).unwrap(),
        "outside metadata"
    );
    assert_eq!(
        fs::read_to_string(outside.0.join("state.json")).unwrap(),
        "outside snapshot"
    );
    assert_eq!(
        serde_json::from_slice::<Session>(&fs::read(retained.join("session.json")).unwrap())
            .unwrap()
            .turns[0]
            .run,
        run.id
    );
}

#[test]
fn project_instructions_use_the_same_pinned_workspace_as_runtime_tools() {
    use std::os::unix::fs::symlink;
    let parent = Directory::new();
    let root = parent.0.join("workspace");
    private_directory(&root).unwrap();
    fs::write(root.join("AGENTS.md"), "original project instructions").unwrap();
    let outside = Directory::new();
    fs::write(
        outside.0.join("AGENTS.md"),
        "outside instructions must not be loaded",
    )
    .unwrap();
    let (events, _) = mpsc::sync_channel(512);
    let tools =
        tools::LocalTools::new(root.clone(), events, Arc::new(AtomicBool::new(false))).unwrap();
    fs::rename(&root, parent.0.join("retained")).unwrap();
    symlink(&outside.0, &root).unwrap();
    let loaded = instructions(&root, &tools).unwrap();
    assert!(loaded.contains("original project instructions"));
    assert!(!loaded.contains("outside instructions must not be loaded"));
}

#[test]
fn source84_catalog_and_saved_conversation_reopen_without_changing_receipts_or_replaying_tools() {
    #[derive(Deserialize)]
    struct LegacyCatalog {
        actor: Actor,
        agent: Agent,
        tools: Vec<Tool>,
    }
    let catalog: LegacyCatalog =
        serde_json::from_str(include_str!("../../tests/fixtures/source84-catalog.json")).unwrap();
    let root = Directory::new();
    let state = Directory::new();
    let directory = SessionDirectory::open(&state.0).unwrap();
    let store = Store::local_file_in(directory.file(), "state.json").unwrap();
    for name in ["local-read", "local-write"] {
        store
            .ensure_policy(
                &catalog.actor.workspace_id,
                name,
                Policy {
                    actors: [catalog.actor.id.clone()].into(),
                    approvers: [catalog.actor.id.clone()].into(),
                    require_approval: false,
                },
            )
            .unwrap();
    }
    for tool in catalog.tools {
        store.ensure_tool(tool).unwrap();
    }
    store.ensure_agent(catalog.agent.clone()).unwrap();
    let session = Session {
        schema_version: 1,
        id: Uuid::parse_str(&catalog.actor.workspace_id).unwrap(),
        workspace: root.0.clone(),
        permission: Permission::FullAccess,
        model: catalog.agent.model,
        endpoint: "http://127.0.0.1:1".into(),
        instructions: catalog.agent.instructions,
        turns: vec![],
    };
    // The independently generated source84 catalog must match immutable v1
    // definitions before a local host can reopen it.
    let mut original = build_host(
        session,
        directory,
        store,
        Box::new(Script {
            responses: vec![
                tool("shell", json!({"command":"printf x >> count"})),
                ModelResponse::Final {
                    output: json!("legacy turn complete"),
                },
            ]
            .into(),
            follow_up: false,
        }),
    )
    .unwrap();
    original.prompt("Legacy task".into()).unwrap();
    original.drive(&mpsc::channel().1).unwrap();
    let old_run = original.active().unwrap();
    assert_eq!(
        original
            .runtime
            .store
            .inspect(&original.actor, old_run)
            .unwrap()
            .status,
        RunStatus::Completed
    );
    let old_receipts = serde_json::to_value(
        original
            .runtime
            .store
            .operations(&original.actor, old_run)
            .unwrap(),
    )
    .unwrap();
    let session = read_session(&original.directory).unwrap();
    drop(original);
    struct ContinuedModel;
    impl ModelExecutor for ContinuedModel {
        fn call(&mut self, request: &ModelRequest) -> Result<ModelResponse, ExecutionError> {
            let history = serde_json::to_string(&request.messages).unwrap();
            for expected in [
                "Legacy task",
                "legacy turn complete",
                "Continue legacy conversation",
            ] {
                assert!(history.contains(expected));
            }
            Ok(ModelResponse::Final {
                output: json!("continued without replay"),
            })
        }
    }
    let directory = SessionDirectory::open(&state.0).unwrap();
    let store = Store::local_file_in(directory.file(), "state.json").unwrap();
    let mut reopened = build_host(session, directory, store, Box::new(ContinuedModel)).unwrap();
    reopened
        .prompt("Continue legacy conversation".into())
        .unwrap();
    reopened.drive(&mpsc::channel().1).unwrap();
    assert_eq!(fs::read_to_string(root.0.join("count")).unwrap(), "x");
    assert_eq!(
        serde_json::to_value(
            reopened
                .runtime
                .store
                .operations(&reopened.actor, old_run)
                .unwrap()
        )
        .unwrap(),
        old_receipts
    );
    assert_eq!(
        reopened
            .runtime
            .store
            .inspect(&reopened.actor, reopened.active().unwrap())
            .unwrap()
            .status,
        RunStatus::Completed
    );
}
