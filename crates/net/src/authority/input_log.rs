//! The input log: every authority tick's `TickInput`, written as it is stepped, and fed back in to reproduce a run.
//!
//! The sim is a function of its seed and the per-tick input (seed 0 is fixed), so replaying the recorded input into the
//! same build reproduces the run, bugs included. A log is a header (the launch arguments and `IW4L_*` environment, so a
//! replay can relaunch itself) followed by length-prefixed tick frames. Ticks are counted from the first stepped tick of
//! the match. Empty ticks are not stored.
//!
//! Recording is on by default (`iw4l-artifacts/inputs/`, the newest few kept); `IW4L_RECORD_INPUT=0` turns it off.
//! `--replay-inputs FILE` replays; once the log runs out the live input takes over again.

use std::collections::VecDeque;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use bevy::prelude::Resource;

use crate::transport::delta::{decode_usercmd, encode_usercmd};
use crate::transport::meta_wire::{decode_actions, encode_actions};
use crate::transport::protocol::{decode_shot_sample, encode_shot_sample};
use crate::transport::wire::{WireError, WireReader, WireWriter};

const MAGIC: &[u8; 8] = b"IW4LINP1";
const KEEP_LOGS: usize = 8;
const FLUSH_EVERY_FRAMES: u32 = 20;
pub const RECORD_FLAG: &str = "--record-inputs";
pub const REPLAY_FLAG: &str = "--replay-inputs";

enum Mode {
    Off,
    Record {
        out: BufWriter<std::fs::File>,
        since_flush: u32,
    },
    Replay {
        frames: VecDeque<(u32, sim::TickInput)>,
        name: String,
        total: usize,
    },
}

#[derive(Resource)]
pub struct InputLog {
    mode: Mode,
    first_tick: Option<u32>,
}

impl Default for InputLog {
    fn default() -> Self {
        Self::from_launch()
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let inline = format!("{flag}=");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(&inline) {
            return Some(value.to_owned());
        }
    }
    None
}

/// Drops the input-log flags (and their values) from a launch command line.
pub fn strip_flags(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut skip = false;
    args.filter(|arg| {
        let drop = skip
            || arg == RECORD_FLAG
            || arg == REPLAY_FLAG
            || arg.starts_with(&format!("{RECORD_FLAG}="))
            || arg.starts_with(&format!("{REPLAY_FLAG}="));
        skip = arg == RECORD_FLAG || arg == REPLAY_FLAG;
        !drop
    })
    .collect()
}

fn strip_cmds(args: Vec<String>) -> Vec<String> {
    let mut skip = false;
    args.into_iter()
        .filter(|arg| {
            let drop = skip || arg == "--cmds" || arg.starts_with("--cmds=");
            skip = arg == "--cmds";
            !drop
        })
        .collect()
}

/// A replay given no `map` of its own gets the launch arguments (and `IW4L_*` environment) the log was recorded with.
pub fn expand_replay_args(args: Vec<String>) -> Vec<String> {
    let Some(path) = flag_value(&args, REPLAY_FLAG) else {
        return args;
    };
    let rest = strip_flags(args.iter().cloned());
    if strip_cmds(rest.clone()).iter().any(|arg| !arg.starts_with("--")) {
        return args;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return args;
    };
    let Ok((lines, _)) = read_header(&bytes) else {
        return args;
    };
    let mut recorded: Vec<String> = Vec::new();
    for line in &lines {
        if let Some(arg) = line.strip_prefix("arg:") {
            recorded.push(arg.to_owned());
        } else if let Some(pair) = line.strip_prefix("env:")
            && let Some((name, value)) = pair.split_once('=')
            && std::env::var_os(name).is_none()
        {
            // SAFETY: called at startup, before any other thread exists.
            unsafe { std::env::set_var(name, value) };
        }
    }
    let mut out = strip_cmds(strip_flags(recorded.into_iter()));
    out.push(REPLAY_FLAG.to_owned());
    out.push(path);
    out.extend(rest);
    out
}

fn inputs_dir() -> PathBuf {
    std::env::var_os("IW4L_ARTIFACTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("iw4l-artifacts"))
        .join("inputs")
}

fn trim_old_logs(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<_> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "iwin"))
        .collect();
    logs.sort_by_key(|e| e.file_name());
    while logs.len() > KEEP_LOGS {
        let oldest = logs.remove(0);
        let _ = std::fs::remove_file(oldest.path());
    }
}

fn write_header(out: &mut impl Write) -> std::io::Result<()> {
    let mut lines: Vec<String> = std::env::args().skip(1).map(|a| format!("arg:{a}")).collect();
    lines.extend(
        std::env::vars()
            .filter(|(name, _)| name.starts_with("IW4L_") && name != "IW4L_CMDS")
            .map(|(name, value)| format!("env:{name}={value}")),
    );
    let mut head = WireWriter::new();
    head.put_bytes(MAGIC);
    head.put_u32(lines.len() as u32);
    for line in &lines {
        head.put_u32(line.len() as u32);
        head.put_bytes(line.as_bytes());
    }
    out.write_all(head.as_slice())
}

fn read_header(bytes: &[u8]) -> Result<(Vec<String>, usize), WireError> {
    let mut input = WireReader::new(bytes);
    let mut magic = [0u8; 8];
    input.get_bytes(&mut magic)?;
    if &magic != MAGIC {
        return Err(WireError::Malformed("not an input log"));
    }
    let count = input.get_u32()? as usize;
    let mut lines = Vec::new();
    for _ in 0..count {
        let len = input.get_u32()? as usize;
        if len > input.remaining() {
            return Err(WireError::Malformed("truncated input log header"));
        }
        let mut text = vec![0u8; len];
        input.get_bytes(&mut text)?;
        lines.push(String::from_utf8_lossy(&text).into_owned());
    }
    Ok((lines, bytes.len() - input.remaining()))
}

fn encode_frame(tick: u32, input: &sim::TickInput) -> Vec<u8> {
    let mut out = WireWriter::new();
    out.put_u32(tick);
    out.put_u16(input.cmds.len() as u16);
    for command in &input.cmds {
        out.put_u32(command.client.0);
        match command.sequence {
            Some(sequence) => {
                out.put_u8(1);
                out.put_u32(sequence.0);
            }
            None => {
                out.put_u8(0);
                out.put_u32(0);
            }
        }
        encode_usercmd(&mut out, &command.command);
    }
    encode_actions(&mut out, &input.actions);
    out.put_u16(input.shot_samples.len() as u16);
    for ((client, time), sample) in &input.shot_samples {
        out.put_u32(client.0);
        out.put_i32(*time);
        encode_shot_sample(&mut out, sample);
    }
    out.finish()
}

fn decode_frame(bytes: &[u8]) -> Result<(u32, sim::TickInput), WireError> {
    let mut input = WireReader::new(bytes);
    let tick = input.get_u32()?;
    let mut cmds = Vec::new();
    for _ in 0..input.get_u16()? {
        let client = sim::ClientId(input.get_u32()?);
        let has_sequence = input.get_u8()? != 0;
        let sequence = input.get_u32()?;
        let command = decode_usercmd(&mut input)?;
        cmds.push(sim::PlayerCommand {
            client,
            command,
            sequence: has_sequence.then_some(sim::CommandSequence(sequence)),
        });
    }
    let actions = decode_actions(&mut input)?;
    let mut shot_samples = Vec::new();
    for _ in 0..input.get_u16()? {
        let client = sim::ClientId(input.get_u32()?);
        let time = input.get_i32()?;
        shot_samples.push(((client, time), decode_shot_sample(&mut input)?));
    }
    Ok((
        tick,
        sim::TickInput {
            cmds,
            actions,
            shot_samples,
        },
    ))
}

impl InputLog {
    fn from_launch() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mode = if let Some(path) = flag_value(&args, REPLAY_FLAG) {
            Self::load(&path)
        } else if let Some(path) = flag_value(&args, RECORD_FLAG) {
            Self::create(PathBuf::from(path))
        } else if std::env::var("IW4L_RECORD_INPUT").as_deref() == Ok("0") {
            Mode::Off
        } else {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            Self::create(inputs_dir().join(format!("input-{stamp}.iwin")))
        };
        Self {
            mode,
            first_tick: None,
        }
    }

    fn create(path: PathBuf) -> Mode {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = match std::fs::File::create(&path) {
            Ok(file) => file,
            Err(error) => {
                diag::warn!(Net, "input log: cannot create {}: {error}", path.display());
                return Mode::Off;
            }
        };
        let mut out = BufWriter::new(file);
        if let Err(error) = write_header(&mut out) {
            diag::warn!(Net, "input log: cannot write {}: {error}", path.display());
            return Mode::Off;
        }
        if let Some(dir) = path.parent() {
            trim_old_logs(dir);
        }
        diag::info!(Net, "input log: recording to {}", path.display());
        Mode::Record {
            out,
            since_flush: 0,
        }
    }

    fn load(path: &str) -> Mode {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                diag::error!(Net, "input log: cannot read {path}: {error}");
                return Mode::Off;
            }
        };
        let (_, mut at) = match read_header(&bytes) {
            Ok(header) => header,
            Err(error) => {
                diag::error!(Net, "input log: {path}: {error:?}");
                return Mode::Off;
            }
        };
        let mut frames = VecDeque::new();
        while at + 4 <= bytes.len() {
            let len =
                u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
            at += 4;
            let Some(body) = bytes.get(at..at + len) else {
                break;
            };
            at += len;
            match decode_frame(body) {
                Ok(frame) => frames.push_back(frame),
                Err(error) => {
                    diag::warn!(Net, "input log: {path}: bad frame ({error:?}), stopping there");
                    break;
                }
            }
        }
        let total = frames.len();
        diag::info!(Net, "input log: replaying {path} ({total} ticks with input)");
        Mode::Replay {
            frames,
            name: path.to_owned(),
            total,
        }
    }

    pub fn replaying(&self) -> bool {
        matches!(&self.mode, Mode::Replay { frames, .. } if !frames.is_empty())
    }

    /// Records this tick's input, or (replaying) swaps in the recorded input for it.
    pub fn step(&mut self, tick: u32, input: &mut sim::TickInput) {
        let first = *self.first_tick.get_or_insert(tick);
        let rel = tick.wrapping_sub(first);
        match &mut self.mode {
            Mode::Off => {}
            Mode::Record { out, since_flush } => {
                if !input.cmds.is_empty() || !input.actions.is_empty() || !input.shot_samples.is_empty() {
                    let body = encode_frame(rel, input);
                    let _ = out.write_all(&(body.len() as u32).to_le_bytes());
                    let _ = out.write_all(&body);
                }
                *since_flush += 1;
                if *since_flush >= FLUSH_EVERY_FRAMES {
                    *since_flush = 0;
                    let _ = out.flush();
                }
            }
            Mode::Replay {
                frames,
                name,
                total,
            } => {
                if frames.is_empty() {
                    return;
                }
                *input = sim::TickInput::default();
                while frames.front().is_some_and(|(at, _)| *at < rel) {
                    frames.pop_front();
                }
                if frames.front().is_some_and(|(at, _)| *at == rel)
                    && let Some((_, recorded)) = frames.pop_front()
                {
                    *input = recorded;
                }
                if frames.is_empty() {
                    diag::info!(
                        Net,
                        "input log: {name} ({total} ticks with input) finished at tick {rel}; live input takes over"
                    );
                }
            }
        }
    }
}
