//! Tools for a human playtester working beside an agent.
//!
//! `playmark [note]` (F8) saves a bundle under `iw4l-artifacts/playtest/<session>/<NNN>/`:
//! the note, the position, a screenshot and state dump (queued as ordinary
//! `screenshot`/`dump` commands) and the tail of the log. `IW4L_CONSOLE_INBOX=<file>`
//! lets another process append console lines to a file that the running game
//! executes; each is logged as `inbox: > <line>`, and what it prints follows in
//! `latest.log`.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use frame::LaunchIdentity;

use crate::feature_dispatch::ConsoleEcho;
use crate::{ConsoleCommand, ConsoleQueue};

const LOG_TAIL_LINES: usize = 300;
const INBOX_POLL: Duration = Duration::from_millis(200);

#[derive(Resource, Default)]
pub(crate) struct PlaytestSession {
    dir: Option<PathBuf>,
    id: String,
    next: u32,
}

#[derive(Resource)]
pub(crate) struct InboxCommands(Mutex<Receiver<String>>);

pub(crate) fn install_inbox(app: &mut App) {
    let Ok(path) = std::env::var("IW4L_CONSOLE_INBOX") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Start at the end of whatever is already there: stale lines from an earlier
    // session must not run again.
    let start = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if fs::OpenOptions::new().create(true).append(true).open(&path).is_err() {
        return;
    }
    let (sender, receiver) = sync_channel(256);
    std::thread::spawn(move || {
        let mut offset = start;
        let mut pending = String::new();
        loop {
            std::thread::sleep(INBOX_POLL);
            let Ok(mut file) = fs::File::open(&path) else {
                continue;
            };
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len < offset {
                offset = 0;
                pending.clear();
            }
            if len == offset || file.seek(SeekFrom::Start(offset)).is_err() {
                continue;
            }
            let mut chunk = Vec::new();
            if file.take(len - offset).read_to_end(&mut chunk).is_err() {
                continue;
            }
            offset += chunk.len() as u64;
            pending.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = pending.find('\n') {
                let line: String = pending.drain(..=end).collect();
                let line = line.trim();
                if !line.is_empty() && sender.send(line.to_owned()).is_err() {
                    return;
                }
            }
        }
    });
    app.insert_resource(InboxCommands(Mutex::new(receiver)));
}

pub(crate) fn drain_inbox(
    inbox: Option<Res<InboxCommands>>,
    mut queue: ResMut<ConsoleQueue>,
) {
    let Some(inbox) = inbox else { return };
    let Ok(receiver) = inbox.0.lock() else { return };
    for line in receiver.try_iter().take(64) {
        diag::info!(Console, "inbox: > {line}");
        queue.push_line(&line);
    }
}

/// F8 is the playtester's "something is wrong here" key.
pub(crate) fn mark_hotkey(keys: Res<ButtonInput<KeyCode>>, mut queue: ResMut<ConsoleQueue>) {
    if keys.just_pressed(KeyCode::F8) {
        queue.push_line("playmark");
    }
}

pub(crate) fn route_mark_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut echo: ConsoleEcho,
    mut session: ResMut<PlaytestSession>,
    mut queue: ResMut<ConsoleQueue>,
    identity: Option<Res<LaunchIdentity>>,
) {
    for cmd in events.read() {
        if cmd.name != "playmark" {
            continue;
        }
        let Some(identity) = identity.as_ref() else {
            echo.write("playmark: launch identity missing (artifacts path unknown)");
            continue;
        };
        if session.dir.is_none() {
            session.id = unix_seconds().to_string();
            session.dir = Some(identity.artifacts.join("playtest").join(&session.id));
        }
        session.next += 1;
        let name = format!("{:03}", session.next);
        let Some(root) = session.dir.clone() else {
            continue;
        };
        let dir = root.join(&name);
        if let Err(error) = fs::create_dir_all(&dir) {
            echo.write(format!("playmark: create {}: {error}", dir.display()));
            continue;
        }
        let shot = format!("playtest/{}/{name}", session.id);
        let dump = format!("playtest_{}_{name}", session.id);
        let note = cmd.args.join(" ");
        let body = format!(
            "mark = {name}\nunix_time = {}\nzone = {}\nnote = {note}\nscreenshot = screenshots/{shot}.png\ndump = dumps/*-{dump}.txt\nlog_tail = log_tail.txt\n",
            unix_seconds(),
            identity.zone,
        );
        if let Err(error) = fs::write(dir.join("note.txt"), body) {
            echo.write(format!("playmark: write note: {error}"));
            continue;
        }
        if let Some(log) = diag::latest_log_path() {
            let _ = fs::write(dir.join("log_tail.txt"), tail_lines(&log, LOG_TAIL_LINES));
        }
        append_index(&root, &name, &note);
        queue.push_line(&format!("screenshot {shot}"));
        queue.push_line(&format!("dump {dump}"));
        echo.write(format!("playmark {name} saved: {}", dir.display()));
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn tail_lines(path: &Path, count: usize) -> String {
    let text = fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

fn append_index(root: &Path, name: &str, note: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("marks.tsv"))
    {
        let _ = writeln!(file, "{name}\t{}\t{note}", unix_seconds());
    }
}
