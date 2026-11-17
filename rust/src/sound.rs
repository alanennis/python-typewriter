//! the margin bell sound

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SOUND_PLAYERS: [&str; 3] = ["paplay", "pw-play", "aplay"];

/// the parts of a wav file pacat needs
#[derive(Debug, PartialEq)]
pub struct Wav {
    pub sample_bytes: u16,
    pub rate: u32,
    pub channels: u16,
    pub frames: Vec<u8>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// read an uncompressed (PCM) wav file
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a wav file".into());
    }
    let mut fmt = None;
    let mut data = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32_at(bytes, i + 4) as usize;
        let body = &bytes[i + 8..(i + 8 + size).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + size + (size & 1); // chunks are padded to an even length
    }
    let fmt = fmt.ok_or("no fmt chunk")?;
    let format = u16_at(fmt, 0);
    if format != 1 && format != 0xFFFE {
        return Err(format!("unsupported wav format {format}"));
    }
    Ok(Wav {
        channels: u16_at(fmt, 2),
        rate: u32_at(fmt, 4),
        sample_bytes: u16_at(fmt, 14).div_ceil(8),
        frames: data.ok_or("no data chunk")?.to_vec(),
    })
}

/// pacat's name for a sample format
fn pacat_format(sample_bytes: u16) -> Result<&'static str, String> {
    match sample_bytes {
        1 => Ok("u8"),
        2 => Ok("s16le"),
        3 => Ok("s24le"),
        4 => Ok("s32le"),
        n => Err(format!("unsupported sample width {n}")),
    }
}

/// plays a wav file with almost no delay.
///
/// starting a player program for every ding takes around a tenth of a
/// second, and the sound card goes to sleep when nothing is playing, which
/// adds more. instead one pacat is started up front with an open stream,
/// and each ding just writes the sound into it.
pub struct StreamingSound {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    frames: Arc<Vec<u8>>,
    playing: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl StreamingSound {
    pub fn new(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let wav = parse_wav(&bytes)?;
        let mut child = Command::new("pacat")
            .args([
                "--playback",
                "--raw",
                &format!("--format={}", pacat_format(wav.sample_bytes)?),
                &format!("--rate={}", wav.rate),
                &format!("--channels={}", wav.channels),
                "--latency-msec=20",
                "--client-name=typewriter",
                "--stream-name=margin bell",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdin = child.stdin.take().ok_or("no pipe to pacat")?;
        Ok(StreamingSound {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            frames: Arc::new(wav.frames),
            playing: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
        })
    }

    /// start the sound; returns false if the player has died
    pub fn play(&mut self) -> bool {
        if !self.alive.load(Ordering::SeqCst) || !matches!(self.child.try_wait(), Ok(None)) {
            self.alive.store(false, Ordering::SeqCst);
            return false;
        }
        // still ringing: don't stack up
        if !self.playing.swap(true, Ordering::SeqCst) {
            let (stdin, frames) = (self.stdin.clone(), self.frames.clone());
            let (playing, alive) = (self.playing.clone(), self.alive.clone());
            // in a thread because the write only finishes as the sound
            // plays, and typing mustn't wait for that
            std::thread::spawn(move || {
                let mut pipe = stdin.lock().unwrap_or_else(|e| e.into_inner());
                if pipe.write_all(&frames).and_then(|_| pipe.flush()).is_err() {
                    alive.store(false, Ordering::SeqCst);
                }
                playing.store(false, Ordering::SeqCst);
            });
        }
        true
    }
}

impl Drop for StreamingSound {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// find a program on the PATH
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_str()?
        .split(':')
        .map(|dir| Path::new(dir).join(name))
        .find(|p| p.is_file())
}

/// the margin bell: a streamed wav, or a player program started each time
pub struct BellSound {
    stream: Option<StreamingSound>,
    player: Option<String>,
    path: PathBuf,
}

impl BellSound {
    /// load sound_file, a relative path being taken from base_dir. wav
    /// files are streamed through pacat for the quickest response; other
    /// files start a player each time. gives None (use the terminal beep)
    /// if there's no file or no way to play it
    pub fn new(sound_file: &str, base_dir: &Path) -> Option<Self> {
        if sound_file.is_empty() {
            return None;
        }
        let mut path = match (sound_file.strip_prefix("~/"), std::env::var_os("HOME")) {
            (Some(rest), Some(home)) => Path::new(&home).join(rest),
            _ => PathBuf::from(sound_file),
        };
        if !path.is_absolute() {
            path = base_dir.join(path);
        }
        if !path.is_file() {
            crate::log_warning(&format!("can't find {}, using terminal beep", path.display()));
            return None;
        }

        let is_wav = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        let stream = if is_wav && which("pacat").is_some() {
            StreamingSound::new(&path)
                .map_err(|e| {
                    crate::log_warning(&format!(
                        "can't stream {} ({e}), starting a player each time",
                        path.display()
                    ))
                })
                .ok()
        } else {
            None
        };
        let player = SOUND_PLAYERS.iter().find(|p| which(p).is_some()).map(|p| p.to_string());
        if stream.is_none() && player.is_none() {
            crate::log_warning("no sound player found, using terminal beep");
            return None;
        }
        Some(BellSound { stream, player, path })
    }

    /// play the sound without waiting for it; false means it couldn't be
    /// played and the caller should beep instead
    pub fn play(&mut self) -> bool {
        if self.stream.as_mut().is_some_and(|s| s.play()) {
            return true;
        }
        let Some(player) = &self.player else { return false };
        match Command::new(player)
            .arg(&self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                // reap it when it finishes so it doesn't linger
                std::thread::spawn(move || child.wait());
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn missing_or_empty_sound_file_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(BellSound::new(dir.path().join("nope.wav").to_str().unwrap(), dir.path()).is_none());
        assert!(BellSound::new("", dir.path()).is_none());
    }

    #[test]
    fn reads_both_ding_files() {
        let dir = crate_dir();
        let w = parse_wav(&std::fs::read(dir.join("ding.wav")).unwrap()).unwrap();
        assert_eq!((w.sample_bytes, w.rate, w.channels), (2, 44100, 1));
        assert_eq!(w.frames.len(), 30869 * 2);
        // ding2 has an extra chunk before the data
        let w = parse_wav(&std::fs::read(dir.join("ding2.wav")).unwrap()).unwrap();
        assert_eq!((w.sample_bytes, w.rate, w.channels), (3, 48000, 1));
        assert_eq!(w.frames.len(), 121240 * 3);
        assert_eq!(pacat_format(w.sample_bytes), Ok("s24le"));
    }

    #[test]
    fn rejects_non_wav() {
        assert!(parse_wav(b"OggS not a wav at all").is_err());
    }
}
