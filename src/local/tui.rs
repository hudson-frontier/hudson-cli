use super::{Command, Error, Event, Host};
use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event as TerminalEvent, KeyCode,
        KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use hudson_core::models::{OperationStatus, RunStatus, WaitReason};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// No model/tool/file content can write terminal control sequences or bidi controls.
fn safe(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for c in text.chars() {
        if (c.is_control() && c != '\n' && c != '\t')
            || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{2028}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        {
            output.extend(c.escape_unicode());
        } else {
            output.push(c);
        }
    }
    output
}
struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
    }
}
const HELP: &str = "Enter: submit · /steer TEXT: redirect active work · /approve or /deny: pending write · /cancel or Ctrl-C: stop · /resume: saved task · /quit: exit · PageUp/Down: scroll";
struct DisplayLog {
    text: String,
    revision: u64,
}
fn push(log: &mut DisplayLog, text: &str) {
    log.text.push_str(&safe(text));
    log.revision = log.revision.wrapping_add(1);
    if log.text.len() > 256 * 1024 {
        let mut at = log.text.len() - 192 * 1024;
        while !log.text.is_char_boundary(at) {
            at += 1;
        }
        log.text.drain(..at);
    }
}
fn wrapped_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut row = String::new();
        let mut columns = 0;
        for grapheme in line.graphemes(true) {
            if grapheme == "\t" {
                for _ in 0..4 - columns % 4 {
                    if columns == width {
                        rows.push(std::mem::take(&mut row));
                        columns = 0;
                    }
                    row.push(' ');
                    columns += 1;
                }
                continue;
            }
            let cells = grapheme.width();
            if !row.is_empty() && columns + cells > width {
                rows.push(std::mem::take(&mut row));
                columns = 0;
            }
            if cells > width {
                // A one-column viewport cannot display a two-column glyph.
                // Keep the row bounded without splitting a grapheme cluster.
                row.push('\u{fffd}');
                columns = 1;
            } else {
                row.push_str(grapheme);
                columns += cells;
            }
        }
        rows.push(row);
    }
    rows
}
#[derive(Default)]
struct ConversationScroll(usize);
impl ConversationScroll {
    fn page_up(&mut self) {
        self.0 = self.0.saturating_add(10);
    }
    fn page_down(&mut self) {
        self.0 = self.0.saturating_sub(10);
    }
    fn visible_rows<'a>(&mut self, rows: &'a [String], height: usize) -> &'a [String] {
        self.0 = self.0.min(rows.len().saturating_sub(height));
        let end = rows.len() - self.0;
        &rows[end.saturating_sub(height)..end]
    }
}
fn append_paste(input: &mut String, text: &str) -> bool {
    if text.len() > (32 * 1024_usize).saturating_sub(input.len()) {
        return false;
    }
    let text = safe(text);
    if input.len() + text.len() > 32 * 1024 {
        return false;
    }
    input.push_str(&text);
    true
}
fn resume_command(workspace: &std::path::Path, id: uuid::Uuid) -> String {
    let workspace = workspace.display().to_string();
    if crate::agent::safe(&workspace) == workspace {
        return format!(
            "Resume: hudson --workspace '{}' --resume {id}",
            workspace.replace('\'', "'\\''")
        );
    }
    // POSIX command substitution removes trailing newlines. A sentinel keeps
    // them intact until the quoted parameter expansion removes that sentinel.
    // Encode every byte so control characters never reach the terminal.
    use std::fmt::Write;
    let mut encoded = String::with_capacity(workspace.len() * 4);
    for byte in workspace.bytes() {
        let _ = write!(encoded, "\\{byte:03o}");
    }
    format!("Resume: hudson_resume_workspace=$(printf '{encoded}x'); hudson --workspace \"${{hudson_resume_workspace%x}}\" --resume {id}")
}
fn send(sender: &mpsc::SyncSender<Command>, command: Command, log: &mut DisplayLog) -> bool {
    match sender.try_send(command) {
        Ok(()) => true,
        Err(_) => {
            push(log, "\nInput queue unavailable or full; try again.\n");
            false
        }
    }
}
pub(super) fn run(
    host: Host,
    receiver: mpsc::Receiver<Event>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), Error> {
    let id = host.session.id;
    let shutdown = host.shutdown.clone();
    let workspace = host.session.workspace.clone();
    let header = safe(&format!(
        "Hudson · code · {} · {:?}\n{}\nSession {}",
        host.session.model,
        host.session.permission,
        host.session.workspace.display(),
        id
    ));
    let store = host.runtime.store.clone();
    let actor = host.actor.clone();
    let mut log = DisplayLog {
        text: format!("{HELP}\n"),
        revision: 0,
    };
    if host.session.permission == crate::local_args::Permission::FullAccess {
        push(
            &mut log,
            "Full access: commands use your OS permissions; no filesystem sandbox.\n",
        );
    }
    if let Some(last) = host.active() {
        if let Ok(history) = store.conversation_history(&actor, last) {
            for message in history.messages {
                for content in message.content {
                    match content {
                        hudson_harness::Content::Json { value } => push(
                            &mut log,
                            &format!(
                                "\n{:?}: {}\n",
                                message.role,
                                value
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| value.to_string())
                            ),
                        ),
                        hudson_harness::Content::Text { text } => {
                            push(&mut log, &format!("\n{:?}: {text}\n", message.role))
                        }
                        _ => {}
                    }
                }
            }
        }
        let view = store.inspect(&actor, last)?;
        push(
            &mut log,
            &format!(
                "\nRestored task {last}: {:?}. /resume continues a safe pending boundary.\n",
                view.status
            ),
        );
    }
    let (sender, commands) = mpsc::sync_channel(32);
    let mut sender = Some(sender);
    enable_raw_mode()?;
    let _screen = Screen;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let worker = std::thread::spawn(move || host.work(commands));
    let mut input = String::new();
    let mut status = "Ready".to_owned();
    let mut scroll = ConversationScroll::default();
    let mut busy = false;
    let mut quitting = false;
    let mut streamed = String::new();
    let mut dirty = true;
    let mut wrapped = Vec::<String>::new();
    let mut wrapped_key = None;
    let result = (|| -> Result<(), Error> {
        loop {
            // Bound each display batch so continuous model output cannot starve
            // keyboard input, cancellation or terminal resize handling.
            for _ in 0..64 {
                let Ok(event) = receiver.try_recv() else {
                    break;
                };
                dirty = true;
                match event {
                    Event::Text(text) => {
                        if streamed.is_empty() {
                            push(&mut log, "\nHudson: ");
                        }
                        if streamed.len() + text.len() <= 128 * 1024 {
                            streamed.push_str(&text);
                        }
                        push(&mut log, &text);
                    }
                    Event::Activity(text) => {
                        streamed.clear();
                        push(&mut log, &format!("\n› {text}\n"));
                    }
                    Event::Error(error) => {
                        push(&mut log, &format!("\nError: {error}\n"));
                        status = "Needs attention".into();
                        busy = false;
                    }
                    Event::State(view) => {
                        busy = !view.status.terminal() && view.status != RunStatus::Waiting;
                        status = format!(
                            "{:?} · calls {} · tools {}",
                            view.status, view.usage.model_calls, view.usage.tool_calls
                        );
                        if let Some(WaitReason::Approval { .. }) = view.wait {
                            for operation in store.operations(&actor, view.id)? {
                                if operation.status == OperationStatus::WaitingApproval {
                                    let details =
                                        store.inspect_operation(&actor, operation.meta.id)?;
                                    push(
                                        &mut log,
                                        &format!(
                                            "\nApproval required: {} {}\nUse /approve or /deny.\n",
                                            details.tool_name.unwrap_or_default(),
                                            details.arguments.unwrap_or_default()
                                        ),
                                    );
                                    break;
                                }
                            }
                        }
                        if view.status.terminal() {
                            if let Some(value) = view.result {
                                if value.as_str() != Some(streamed.as_str()) {
                                    push(
                                        &mut log,
                                        &format!(
                                            "\n\nHudson: {}\n",
                                            value
                                                .as_str()
                                                .map(str::to_owned)
                                                .unwrap_or_else(|| value.to_string())
                                        ),
                                    );
                                } else {
                                    push(&mut log, "\n");
                                }
                            }
                            streamed.clear();
                            if let Some(reason) = view.reason {
                                push(&mut log, &format!("\n{reason}\n"));
                            }
                        }
                    }
                }
            }
            if quitting && worker.is_finished() {
                break;
            }
            if worker.is_finished() {
                return Err("local host exited unexpectedly".into());
            }
            if dirty {
                terminal.draw(|frame| {
                    let layout = Layout::vertical([
                        Constraint::Length(4),
                        Constraint::Min(3),
                        Constraint::Length(3),
                        Constraint::Length(2),
                    ])
                    .split(frame.area());
                    frame.render_widget(
                        Paragraph::new(header.as_str()).style(Style::default().fg(Color::Cyan)),
                        layout[0],
                    );
                    let width = layout[1].width.saturating_sub(2).max(1) as usize;
                    if wrapped_key != Some((log.revision, width)) {
                        wrapped = wrapped_lines(&log.text, width);
                        wrapped_key = Some((log.revision, width));
                    }
                    let height = layout[1].height.saturating_sub(2) as usize;
                    frame.render_widget(
                        Paragraph::new(scroll.visible_rows(&wrapped, height).join("\n"))
                            .block(Block::default().borders(Borders::ALL).title("Conversation")),
                        layout[1],
                    );
                    frame.render_widget(
                        Paragraph::new(safe(&input))
                            .wrap(Wrap { trim: false })
                            .block(Block::default().borders(Borders::ALL).title(if busy {
                                "Working · type /steer or /cancel"
                            } else {
                                "Task"
                            })),
                        layout[2],
                    );
                    frame.render_widget(
                        Paragraph::new(format!("{} · Ctrl-C cancel · /quit exit", safe(&status))),
                        layout[3],
                    );
                })?;
                dirty = false;
            }
            if !event::poll(Duration::from_millis(40))? {
                continue;
            }
            dirty = true;
            match event::read()? {
                TerminalEvent::Paste(text) if !quitting => {
                    if !append_paste(&mut input, &text) {
                        push(&mut log, "\nPaste exceeds the 32 KiB input limit.\n");
                    }
                }
                TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        cancelled.store(true, Ordering::Relaxed);
                        if let Some(sender) = &sender {
                            send(sender, Command::Stop, &mut log);
                        }
                        status = "Cancelling; model IO may wait for its bounded transport timeout"
                            .into();
                    }
                    KeyCode::PageUp => scroll.page_up(),
                    KeyCode::PageDown => scroll.page_down(),
                    KeyCode::Backspace if !quitting => {
                        input.pop();
                    }
                    KeyCode::Char(c)
                        if !quitting
                            && !key.modifiers.contains(KeyModifiers::CONTROL)
                            && input.len() + c.len_utf8() <= 32 * 1024 =>
                    {
                        input.push(c)
                    }
                    KeyCode::Enter if !quitting => {
                        let text = std::mem::take(&mut input);
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }
                        scroll = ConversationScroll::default();
                        push(&mut log, &format!("\nYou: {text}\n"));
                        if text == "/quit" {
                            shutdown.store(true, Ordering::Release);
                            cancelled.store(true, Ordering::Relaxed);
                            sender.take();
                            quitting = true;
                            status = "Stopping local host…".into();
                            continue;
                        }
                        if text == "/help" {
                            push(&mut log, &format!("\n{HELP}\n"));
                            continue;
                        }
                        let command = match text {
                            "/cancel" => {
                                cancelled.store(true, Ordering::Relaxed);
                                Command::Stop
                            }
                            "/approve" => Command::Approve(true),
                            "/deny" => Command::Approve(false),
                            "/resume" => Command::Resume,
                            _ if text.starts_with("/steer ") => {
                                Command::Steer(text[7..].to_owned())
                            }
                            _ if text.starts_with('/') => {
                                push(&mut log, "\nUnknown command; /help lists controls.\n");
                                continue;
                            }
                            _ if busy => {
                                push(&mut log, "\nUse /steer TEXT while a task is active.\n");
                                continue;
                            }
                            _ => Command::Prompt(text.into()),
                        };
                        if let Some(sender) = &sender {
                            if send(sender, command, &mut log) {
                                busy = true;
                                status = "Working…".into();
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(())
    })();
    shutdown.store(true, Ordering::Release);
    cancelled.store(true, Ordering::Relaxed);
    drop(sender);
    // Drain bounded worker output while waiting, avoiding a shutdown deadlock.
    while !worker.is_finished() {
        let _ = receiver.recv_timeout(Duration::from_millis(50));
    }
    worker.join().map_err(|_| "local host worker panicked")?;
    result?;
    drop(_screen);
    println!("{}", resume_command(&workspace, id));
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn scrolling_reaches_soft_wrapped_history_and_clamps_after_resize_or_trim() {
        let text: String = (0..40).map(|index| format!("{index:04}")).collect();
        let rows = super::wrapped_lines(&text, 4);
        assert_eq!(text.lines().count(), 1);
        let mut scroll = super::ConversationScroll::default();
        scroll.page_up();
        assert_eq!(
            scroll.visible_rows(&rows, 5),
            ["0025", "0026", "0027", "0028", "0029"]
        );
        scroll.page_down();
        assert_eq!(scroll.visible_rows(&rows, 5), &rows[35..]);
        for _ in 0..10 {
            scroll.page_up();
        }
        assert_eq!(
            scroll.visible_rows(&rows, 5),
            ["0000", "0001", "0002", "0003", "0004"]
        );
        assert_eq!(scroll.visible_rows(&rows, 50), rows);
        scroll.page_up();
        assert_eq!(scroll.visible_rows(&rows[..2], 5), &rows[..2]);
        assert_eq!(scroll.visible_rows(&[], 5), &[] as &[String]);
    }

    #[test]
    fn terminal_content_cannot_inject_controls() {
        let value = super::safe("hello\n\x1b]52;c;secret\x07\u{202e}abc");
        assert!(!value.contains('\x1b'));
        assert!(!value.contains('\u{202e}'));
        assert!(value.contains("hello\n"));
    }

    #[test]
    fn paste_bound_includes_control_character_expansion() {
        let mut input = "x".repeat(32 * 1024 - 5);
        assert!(!super::append_paste(&mut input, "\x1b"));
        assert_eq!(input.len(), 32 * 1024 - 5);
        assert!(super::append_paste(&mut input, "hello"));
        assert_eq!(input.len(), 32 * 1024);
        assert!(!super::append_paste(&mut input, "a"));
    }

    #[test]
    fn resume_display_escapes_workspace_controls_and_quotes() {
        for workspace in ["/tmp/a'b", "/tmp/a'\n\x1b\u{202e}", "/tmp/trailing\n"] {
            let command = super::resume_command(std::path::Path::new(workspace), uuid::Uuid::nil());
            assert!(!command.chars().any(char::is_control));
            assert!(!command.contains('\u{202e}'));
            let shell = format!(
                "hudson() {{ printf '%s' \"$2\"; }}; {}",
                command.strip_prefix("Resume: ").unwrap()
            );
            let output = std::process::Command::new("/bin/sh")
                .args(["-c", &shell])
                .env_clear()
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(output.stdout, workspace.as_bytes());
        }
    }

    #[test]
    fn wrapping_uses_terminal_columns_and_keeps_graphemes_together() {
        assert_eq!(super::wrapped_lines("界a界", 3), ["界a", "界"]);
        assert_eq!(
            super::wrapped_lines("e\u{301}e\u{301}x", 2),
            ["e\u{301}e\u{301}", "x"]
        );
        assert_eq!(super::wrapped_lines("👩‍💻a🙂", 3), ["👩‍💻a", "🙂"]);
        assert_eq!(super::wrapped_lines("a\n\nb", 3), ["a", "", "b"]);
        assert_eq!(super::wrapped_lines("\tx", 3), ["   ", " x"]);
        assert_eq!(super::wrapped_lines("界", 1), ["\u{fffd}"]);
    }
}
