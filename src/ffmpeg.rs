use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::cli::{Label, LabelPosition};
use crate::output;
use crate::platform::Platform;

/// Configuration for an ffmpeg capture session.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub output_path: String,
    pub duration: Option<Duration>,
    pub interval: Option<Duration>,
    pub verbose: bool,
    /// Where the screen and audio are recorded from.
    pub devices: CaptureDevices,
}

/// The screen and audio devices one capture session records from.
///
/// Audio devices are named the way the screen's platform names them to ffmpeg:
/// an avfoundation audio index on macOS, a PulseAudio source name on Linux, a
/// DirectShow device name on Windows. Either may be absent; a capture session
/// records the audio that is available, down to none at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureDevices {
    pub screen: Screen,
    pub system_audio: Option<String>,
    pub microphone: Option<String>,
}

/// The ffmpeg input device that grabs the screen. It also decides the input
/// device the audio comes through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// macOS: `avfoundation`, for the screen and for audio.
    Avfoundation,
    /// Linux: `x11grab` on an X11 display, with audio through `pulse`.
    X11 { display: String },
    /// Windows: `gdigrab` on the whole desktop, with audio through `dshow`.
    Gdi,
}

impl CaptureConfig {
    pub fn new(output_path: String, devices: CaptureDevices) -> Self {
        Self {
            output_path,
            duration: None,
            interval: None,
            verbose: false,
            devices,
        }
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

/// Format a `Duration` as seconds with three decimals (`10.500`), the
/// fractional-seconds form ffmpeg's `-t` and `-segment_time` expect.
fn format_seconds(duration: Duration) -> String {
    format!("{:.3}", duration.as_secs_f64())
}

/// Packets an x11grab, gdigrab, pulse, or dshow input may queue while the
/// encoder catches up. ffmpeg's small default makes a live audio input drop
/// samples behind the much slower screen input.
const LIVE_INPUT_QUEUE_SIZE: &str = "1024";

/// One `-i` of a capture command.
struct CaptureInput {
    /// The ffmpeg input device, given to `-f`.
    format: &'static str,
    /// What the device opens, given to `-i`.
    target: String,
    has_audio: bool,
}

/// The inputs a capture command opens for `devices`: the screen first, as
/// input 0, then each audio device, system audio before the microphone.
fn capture_inputs(devices: &CaptureDevices) -> Vec<CaptureInput> {
    let mut audio = [&devices.system_audio, &devices.microphone]
        .into_iter()
        .flatten();
    let input = |format: &'static str, target: String, has_audio: bool| CaptureInput {
        format,
        target,
        has_audio,
    };

    match &devices.screen {
        Screen::Avfoundation => {
            // The screen input carries the first audio device too: the
            // "video:audio" selector binds a video index to an audio index in
            // one avfoundation grab, and "1:none" skips audio. Any further
            // audio device is an audio-only input, ":<index>".
            let bound = audio.next();
            let selector = format!("1:{}", bound.map_or("none", String::as_str));
            std::iter::once(input("avfoundation", selector, bound.is_some()))
                .chain(audio.map(|device| input("avfoundation", format!(":{}", device), true)))
                .collect()
        }
        Screen::X11 { display } => std::iter::once(input("x11grab", display.clone(), false))
            .chain(audio.map(|device| input("pulse", device.clone(), true)))
            .collect(),
        Screen::Gdi => std::iter::once(input("gdigrab", "desktop".to_string(), false))
            .chain(audio.map(|device| input("dshow", format!("audio={}", device), true)))
            .collect(),
    }
}

/// Build the `-filter_complex` graph that brings every audio input to `[aout]`.
///
/// Capture devices run on independent clocks; without per-input aresample they
/// drift out of sync over time. `aresample=async=1` is the modern replacement
/// for the deprecated `-async` flag. Two inputs are then mixed into one track.
fn build_audio_filter(audio_inputs: &[usize]) -> String {
    if let [only] = audio_inputs {
        return format!("[{}:a]aresample=async=1:first_pts=0[aout]", only);
    }

    let mut filter = String::new();
    for (n, input) in audio_inputs.iter().enumerate() {
        filter.push_str(&format!(
            "[{}:a]aresample=async=1:first_pts=0[a{}];",
            input, n
        ));
    }
    for n in 0..audio_inputs.len() {
        filter.push_str(&format!("[a{}]", n));
    }
    filter.push_str(&format!(
        "amix=inputs={}:duration=longest[aout]",
        audio_inputs.len()
    ));
    filter
}

/// Build an ffmpeg command for screen + (optional) audio capture.
///
/// The screen is input 0 and each audio device follows it — on macOS the
/// first audio device shares the screen's avfoundation input instead. Every
/// audio input is resampled and, when there are two, mixed with
/// `-filter_complex amix`. Output is MP4 with H.264 video and, when any audio
/// device is present, AAC audio.
pub fn build_capture_command(config: &CaptureConfig) -> Command {
    let mut cmd = Command::new("ffmpeg");

    cmd.args(["-y"]);

    // macOS keeps exactly the command it had before Linux and Windows support
    // (ADR 0004), so the options those two need are added only for them.
    let on_macos = config.devices.screen == Screen::Avfoundation;

    let inputs = capture_inputs(&config.devices);
    for input in &inputs {
        if !on_macos {
            cmd.args(["-thread_queue_size", LIVE_INPUT_QUEUE_SIZE]);
        }
        cmd.args(["-f", input.format, "-i", &input.target]);
    }

    let audio_inputs: Vec<usize> = inputs
        .iter()
        .enumerate()
        .filter(|(_, input)| input.has_audio)
        .map(|(index, _)| index)
        .collect();
    if !audio_inputs.is_empty() {
        cmd.args(["-filter_complex", &build_audio_filter(&audio_inputs)]);

        // Map: screen video from input 0, the resampled or mixed audio from
        // [aout].
        cmd.args(["-map", "0:v", "-map", "[aout]"]);

        // Audio codec: AAC.
        cmd.args(["-c:a", "aac", "-b:a", "128k"]);
    }

    // Video codec: H.264
    cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);

    // x11grab and gdigrab hand over RGB frames, for which libx264 would keep
    // full 4:4:4 chroma: a High 4:4:4 stream that most players, browsers and
    // Windows' own included, refuse to open. 4:2:0 plays everywhere.
    if !on_macos {
        cmd.args(["-pix_fmt", "yuv420p"]);
    }

    // Duration limit
    if let Some(duration) = config.duration {
        cmd.args(["-t", &format_seconds(duration)]);
    }

    // Interval mode: use segment muxer. The flags below give us a seamless
    // split: each segment starts at t=0 (playable in any MP4 player) and
    // keyframes are forced exactly at the segment boundary so the muxer can
    // cut without losing frames.
    let output_path = if let Some(interval) = config.interval {
        let interval_secs = format_seconds(interval);
        cmd.args([
            "-f",
            "segment",
            "-segment_time",
            &interval_secs,
            "-reset_timestamps",
            "1",
            "-force_key_frames",
            &format!("expr:gte(t,n_forced*{})", interval_secs),
        ]);
        let base = Path::new(&config.output_path);
        output::segment_ffmpeg_pattern(base).to_string_lossy().to_string()
    } else {
        config.output_path.clone()
    };
    cmd.arg(&output_path);

    cmd
}

/// Configuration for an ffmpeg cut (extract a range from a source video).
#[derive(Debug, Clone)]
pub struct CutConfig {
    pub source_path: PathBuf,
    pub start: Duration,
    pub length: Duration,
    pub output_path: PathBuf,
    pub fast: bool,
}

impl CutConfig {
    pub fn new(source_path: &Path, start: Duration, length: Duration, output_path: &Path) -> Self {
        Self {
            source_path: source_path.to_path_buf(),
            start,
            length,
            output_path: output_path.to_path_buf(),
            fast: false,
        }
    }

    pub fn with_fast(mut self, fast: bool) -> Self {
        self.fast = fast;
        self
    }
}

/// Build an ffmpeg command that extracts a cut range from a source video.
///
/// Default (re-encode, frame-accurate):
///   ffmpeg -y -ss <from> -i <source> -t <length> \
///     -c:v libx264 -preset ultrafast -crf 23 \
///     -c:a aac -b:a 128k <output>
///
/// `--fast` (stream copy, keyframe-aligned):
///   ffmpeg -y -ss <from> -i <source> -t <length> \
///     -c copy -avoid_negative_ts make_zero <output>
///
/// `-ss` goes before `-i` in both cases for fast seeking. Because the default
/// re-encodes, the cut is still frame-accurate despite the fast seek.
pub fn build_cut_command(config: &CutConfig) -> Command {
    let mut cmd = Command::new("ffmpeg");

    cmd.args(["-y"]);
    cmd.args(["-ss", &format_seconds(config.start)]);
    cmd.arg("-i").arg(&config.source_path);
    cmd.args(["-t", &format_seconds(config.length)]);

    if config.fast {
        cmd.args(["-c", "copy", "-avoid_negative_ts", "make_zero"]);
    } else {
        cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);
        cmd.args(["-c:a", "aac", "-b:a", "128k"]);
    }

    cmd.arg(&config.output_path);

    cmd
}

/// Configuration for an ffmpeg label pass (draw timed text onto a video).
#[derive(Debug, Clone)]
pub struct LabelConfig {
    pub source_path: PathBuf,
    pub labels: Vec<Label>,
    pub output_path: PathBuf,
    /// Font file to draw with. `None` lets ffmpeg resolve the system font.
    pub font: Option<PathBuf>,
}

impl LabelConfig {
    pub fn new(source_path: &Path, labels: Vec<Label>, output_path: &Path) -> Self {
        Self {
            source_path: source_path.to_path_buf(),
            labels,
            output_path: output_path.to_path_buf(),
            font: None,
        }
    }

    pub fn with_font(mut self, font: Option<PathBuf>) -> Self {
        self.font = font;
        self
    }
}

/// Distance from the top or bottom edge to a label's background, as a
/// fraction of the frame height. Proportional so a label sits the same
/// distance in whatever the source resolution is.
const LABEL_MARGIN_FRACTION: f64 = 0.05;

/// Padding drawn around a label's text to make its background a band rather
/// than a tight outline. The font size divided by this is the padding in
/// pixels, so a bigger label gets a proportionally bigger band.
const LABEL_PADDING_DIVISOR: u32 = 3;

/// Build an ffmpeg command that draws every label onto a source video.
///
///   ffmpeg -y -i <source> -vf <drawtext chain> \
///     -c:v libx264 -preset ultrafast -crf 23 -c:a aac -b:a 128k <output>
///
/// One `drawtext` filter per label, chained in the order the labels were
/// given, each gated to its own label window by `enable=between(...)`. The
/// video is always re-encoded — drawing on frames is what the command is for —
/// so the output is the same H.264/AAC MP4 the rest of the tool produces.
pub fn build_label_command(config: &LabelConfig) -> Command {
    let mut cmd = Command::new("ffmpeg");

    cmd.args(["-y"]);
    cmd.arg("-i").arg(&config.source_path);
    cmd.arg("-vf")
        .arg(build_label_filter(&config.labels, config.font.as_deref()));

    cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);
    cmd.args(["-c:a", "aac", "-b:a", "128k"]);

    cmd.arg(&config.output_path);

    cmd
}

/// Build the `drawtext` chain for `labels`, one filter per label.
pub fn build_label_filter(labels: &[Label], font: Option<&Path>) -> String {
    labels
        .iter()
        .map(|label| build_drawtext_filter(label, font))
        .collect::<Vec<_>>()
        .join(",")
}

/// Build the single `drawtext` filter that draws one label.
fn build_drawtext_filter(label: &Label, font: Option<&Path>) -> String {
    let padding = match label.background {
        // No background means no band to pad, so the text sits right on the
        // margin.
        None => 0,
        Some(_) => label.size / LABEL_PADDING_DIVISOR,
    };

    // Horizontally centred; vertically a margin in from the chosen edge, with
    // the padding added so the background never overhangs the frame.
    let y = match label.position {
        LabelPosition::Top => format!("h*{}+{}", LABEL_MARGIN_FRACTION, padding),
        LabelPosition::Bottom => format!("h-text_h-h*{}-{}", LABEL_MARGIN_FRACTION, padding),
    };

    // A label's text is literal: `expansion=none` stops drawtext reading a
    // `%{...}` in it as an ffmpeg expression, and keeps the escaping below the
    // same for the text and for the font path.
    let mut filter = String::from("drawtext=expansion=none:");
    if let Some(font) = font {
        filter.push_str(&format!(
            "fontfile={}:",
            escape_filter_value(&font.to_string_lossy())
        ));
    }
    filter.push_str(&format!("text={}", escape_filter_value(&label.text)));
    filter.push_str(&format!(":fontcolor={}", label.color));
    filter.push_str(&format!(":fontsize={}", label.size));
    filter.push_str(":x=(w-text_w)/2");
    filter.push_str(&format!(":y={}", y));
    if let Some(background) = &label.background {
        filter.push_str(&format!(
            ":box=1:boxcolor={}:boxborderw={}",
            background, padding
        ));
    }
    filter.push_str(&format!(
        ":enable='between(t,{},{})'",
        format_seconds(label.start),
        format_seconds(label.end())
    ));

    filter
}

/// Escape a value so ffmpeg's filtergraph parser hands it to a filter option
/// verbatim.
///
/// The value is read twice over — the graph is split into filters and their
/// options, then each option value is unescaped — and every pass that treats a
/// character as syntax eats one backslash. So the count a character needs is
/// set by how many passes it has to survive, and a backslash written for the
/// inner pass has to survive the outer one itself. Each count below was
/// verified by rendering the character and reading it back off the frame:
///
/// - `,` `;` `[` `]` end a filter or delimit a stream label: one backslash.
/// - `:` ends an option, and its backslash is then eaten by the option pass:
///   two.
/// - `'` quotes an option value: three.
/// - `\` is the escape character in both passes, so it doubles in each: four.
///
/// There is no arm for `%` because [`build_drawtext_filter`] sets
/// `expansion=none`. Letting `drawtext` expand its own text would add a third
/// pass, and with it a backslash to every count here.
fn escape_filter_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str(r"\\\\"),
            '\'' => escaped.push_str(r"\\\'"),
            ':' => escaped.push_str(r"\\:"),
            ',' | ';' | '[' | ']' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Parse the last `time=` token from ffmpeg stderr progress output.
///
/// ffmpeg emits lines like `time=00:00:15.00` while encoding, separating
/// progress updates with a carriage return rather than a newline, so several
/// tokens can share one `\n`-delimited line. Returns the last one — the length
/// actually written, which `cut` and `label` both compare against what the
/// user asked for.
pub fn parse_written_length(stderr: &str) -> Option<Duration> {
    stderr.split(['\n', '\r']).rev().find_map(|chunk| {
        let token: String = chunk
            .split_once("time=")?
            .1
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ':' || *c == '.')
            .collect();
        // ffmpeg writes `HH:MM:SS.mmm`; a bare seconds token needs a unit
        // before the shared timespec parser will take it.
        let timespec = if token.contains(':') {
            token
        } else {
            format!("{}s", token)
        };
        crate::cli::parse_timespec(&timespec).ok()
    })
}

/// Run a one-shot ffmpeg command to completion, returning its exit status and
/// everything it wrote to stderr.
///
/// stderr is always captured — callers scrape progress out of it — and, when
/// `echo_stderr`, forwarded to our own stderr as it arrives so a long run still
/// shows ffmpeg's progress live. The child gets no stdin: ffmpeg treats stdin
/// as a keyboard, and a stray `q` would stop it early.
pub fn run_to_completion(
    mut cmd: Command,
    echo_stderr: bool,
) -> std::io::Result<(ExitStatus, String)> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut child_stderr = child.stderr.take().expect("stderr was piped");

    let mut captured: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = match child_stderr.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                // Leave no orphan still writing to the output file: the caller
                // deletes that file on error.
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        if echo_stderr {
            let _ = std::io::stderr().write_all(&buffer[..read]);
        }
        captured.extend_from_slice(&buffer[..read]);
    }

    let status = child.wait()?;
    Ok((status, String::from_utf8_lossy(&captured).into_owned()))
}

/// A device exposed by macOS's AVFoundation layer (avfoundation input in ffmpeg).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvfoundationDevice {
    pub index: usize,
    pub name: String,
}

/// The kind of device section we are currently parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceSection {
    None,
    Video,
    Audio,
}

/// Parse the textual output of `ffmpeg -f avfoundation -list_devices true -i ""`.
///
/// ffmpeg writes the listing on stderr. Lines look like:
/// `   [AVFoundation indev @ 0x...] AVFoundation video devices:`
/// `   [AVFoundation indev @ 0x...] [0] FaceTime HD Camera`
///
/// The line that announces a section ("AVFoundation video devices:" /
/// "AVFoundation audio devices:") is followed by entries tagged with `[N]`. We
/// collect (index, name) tuples, tracking whether we are inside a video or
/// audio section.
pub fn parse_avfoundation_listing(output: &str) -> (Vec<AvfoundationDevice>, Vec<AvfoundationDevice>) {
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut section = DeviceSection::None;

    for line in output.lines() {
        let line = line.trim();

        // Every line ffmpeg emits from the listing has a "[AVFoundation indev
        // @ 0xADDR]" prefix; the body after that prefix is what carries the
        // real content.
        let body = match strip_device_log_prefix(line) {
            Some(b) => b,
            None => continue,
        };

        // Section headers appear inside the body, e.g.
        // "AVFoundation video devices:" — detect them first so that the
        // section is set before any subsequent device entry is parsed.
        if body.starts_with("AVFoundation video devices:") {
            section = DeviceSection::Video;
            continue;
        }
        if body.starts_with("AVFoundation audio devices:") {
            section = DeviceSection::Audio;
            continue;
        }

        // Device entry: "[<index>] <name>".
        let (idx, rest) = match body.strip_prefix('[').and_then(|s| s.split_once(']')) {
            Some(parts) => parts,
            None => continue,
        };
        let index: usize = match idx.trim().parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let name = rest.trim().trim_start_matches('[').trim().to_string();

        match section {
            DeviceSection::Video => video.push(AvfoundationDevice { index, name }),
            DeviceSection::Audio => audio.push(AvfoundationDevice { index, name }),
            DeviceSection::None => {}
        }
    }

    (video, audio)
}

fn strip_device_log_prefix(line: &str) -> Option<&str> {
    // Lines like "[AVFoundation indev @ 0xc97014140] [0] BlackHole 2ch" or
    // "[dshow @ 000001e5d6a8b2c0] "Microphone (Realtek(R) Audio)" (audio)".
    // Skip past the first "]" to get to the device entry.
    let close = line.find(']')?;
    let body = line[close + 1..].trim_start();
    Some(body)
}

/// Run one of ffmpeg's device-listing invocations, turning a missing ffmpeg
/// into the platform's install instructions.
fn run_device_listing(args: &[&str]) -> anyhow::Result<std::process::Output> {
    Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(Platform::current().ffmpeg_not_found_message())
            } else {
                e.into()
            }
        })
}

/// Run `ffmpeg -f avfoundation -list_devices true -i ""` and return the parsed
/// device listing. Returns an error if ffmpeg is missing, cannot be invoked,
/// or fails to run.
pub fn detect_avfoundation_devices() -> anyhow::Result<(Vec<AvfoundationDevice>, Vec<AvfoundationDevice>)> {
    let output = run_device_listing(&["-f", "avfoundation", "-list_devices", "true", "-i", ""])?;

    // ffmpeg prints the listing on stderr (and exits with code 1 because the
    // empty input is invalid). We accept any non-error spawn; decode stderr as
    // UTF-8 and parse it.
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_avfoundation_listing(&stderr))
}

/// Find the screen and audio devices to record from on `platform`.
///
/// Fails only when there is nothing to record: ffmpeg is missing, or on Linux
/// there is no X11 display. A missing audio device is not an error; it is left
/// out of the result, and [`missing_audio_warnings`] says what the recording
/// will be without.
pub fn detect_capture_devices(platform: Platform) -> anyhow::Result<CaptureDevices> {
    match platform {
        Platform::MacOs => {
            let (_video, audio) = detect_avfoundation_devices()?;
            Ok(CaptureDevices {
                screen: Screen::Avfoundation,
                system_audio: find_blackhole_index(&audio).map(|index| index.to_string()),
                microphone: find_microphone_index(&audio).map(|index| index.to_string()),
            })
        }
        Platform::Linux => {
            let display = x11_display(std::env::var("DISPLAY").ok())?;
            // `-sources` prints the listing on stdout. Without a PulseAudio or
            // PipeWire server it prints no sources, and we record no audio.
            let output = run_device_listing(&["-hide_banner", "-sources", "pulse"])?;
            let sources = parse_device_sources(&String::from_utf8_lossy(&output.stdout));
            Ok(CaptureDevices {
                screen: Screen::X11 { display },
                system_audio: find_pulse_system_audio(&sources),
                microphone: find_pulse_microphone(&sources),
            })
        }
        Platform::Windows => {
            let output = run_device_listing(&[
                "-hide_banner",
                "-list_devices",
                "true",
                "-f",
                "dshow",
                "-i",
                "dummy",
            ])?;
            let audio = parse_dshow_audio_devices(&String::from_utf8_lossy(&output.stderr));
            Ok(CaptureDevices {
                screen: Screen::Gdi,
                system_audio: find_dshow_loopback(&audio),
                microphone: find_dshow_microphone(&audio),
            })
        }
    }
}

/// The X11 display to record, from the value of `$DISPLAY`.
fn x11_display(display: Option<String>) -> anyhow::Result<String> {
    match display.filter(|display| !display.is_empty()) {
        Some(display) => Ok(display),
        None => anyhow::bail!(
            "No X11 display to record: DISPLAY is not set.\n\
             On Linux, vidcapture start records an X11 display with ffmpeg's x11grab;\n\
             run it from inside a desktop session."
        ),
    }
}

/// Warnings to show before a capture session starts, for what it will not
/// record.
pub fn capture_warnings(devices: &CaptureDevices) -> Vec<String> {
    let mut warnings = Vec::new();
    if matches!(devices.screen, Screen::X11 { .. })
        && is_wayland_session(std::env::var("XDG_SESSION_TYPE").ok().as_deref())
    {
        warnings.push(String::from(
            "This is a Wayland session. x11grab only sees X11 (XWayland) windows, \
             so other windows may record as black. Log in to an X11 session to \
             record the whole screen.",
        ));
    }
    warnings.extend(missing_audio_warnings(devices));
    warnings
}

/// True if `$XDG_SESSION_TYPE` says the desktop runs on Wayland rather than X11.
fn is_wayland_session(session_type: Option<&str>) -> bool {
    session_type.is_some_and(|session_type| session_type.eq_ignore_ascii_case("wayland"))
}

/// One warning per audio source a capture session on `devices` will not
/// record, saying how to get it where there is a way.
pub fn missing_audio_warnings(devices: &CaptureDevices) -> Vec<String> {
    let mut warnings = Vec::new();
    if devices.system_audio.is_none() {
        let reason = match devices.screen {
            Screen::Avfoundation => {
                "BlackHole 2ch was not found. Run `vidcapture help` to set it up"
            }
            Screen::X11 { .. } => "no PulseAudio or PipeWire output monitor was found",
            Screen::Gdi => {
                "no loopback device such as Stereo Mix was found. \
                 Run `vidcapture help` to set one up"
            }
        };
        warnings.push(format!("System audio is not recorded: {}.", reason));
    }
    if devices.microphone.is_none() {
        warnings.push(String::from(
            "Microphone audio is not recorded: no microphone was found.",
        ));
    }
    warnings
}

/// Find the index of the BlackHole 2ch audio device, if present.
///
/// The match is strict on the channel count: we want "BlackHole 2ch", not the
/// 16-channel variant. The name match is otherwise case-insensitive so both
/// "BlackHole 2ch" and "Blackhole 2ch" are recognised.
pub fn find_blackhole_index(devices: &[AvfoundationDevice]) -> Option<usize> {
    devices
        .iter()
        .find(|d| is_blackhole_2ch(&d.name))
        .map(|d| d.index)
}

/// True if `name` refers to the 2-channel BlackHole device specifically.
fn is_blackhole_2ch(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Match "blackhole 2ch" exactly, allowing extra whitespace but not another
    // channel suffix like "16ch" or "64ch".
    let trimmed = lower.trim();
    trimmed == "blackhole 2ch"
}

/// Find the index of the user's microphone among avfoundation audio devices,
/// skipping BlackHole. See [`pick_microphone`].
pub fn find_microphone_index(devices: &[AvfoundationDevice]) -> Option<usize> {
    let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
    pick_microphone(&names, is_blackhole_2ch).map(|position| devices[position].index)
}

/// Pick the microphone out of a platform's audio input names, returning its
/// position in `names`.
///
/// We prefer devices whose name includes "Microphone" / "Mic"
/// (case-insensitive) but never a loopback device, so a loopback with
/// "Microphone" in its name would still be skipped. If no such device exists,
/// fall back to the first audio input that is not a loopback — this covers
/// hosts where the mic name doesn't include "Microphone" (e.g. some USB
/// interfaces labelled by vendor model).
fn pick_microphone(names: &[&str], is_loopback: fn(&str) -> bool) -> Option<usize> {
    let named_mic = names.iter().position(|name| {
        let lower = name.to_lowercase();
        !is_loopback(name) && (lower.contains("microphone") || lower.starts_with("mic "))
    });
    named_mic.or_else(|| names.iter().position(|name| !is_loopback(name)))
}

/// One entry of the listing `ffmpeg -sources <device>` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceSource {
    /// What ffmpeg's `-i` takes to open the source.
    pub name: String,
    pub description: String,
    /// Whether the device reports this source as its default.
    pub is_default: bool,
}

/// Parse the textual output of `ffmpeg -sources <device>`.
///
/// ffmpeg writes the listing on stdout, one source per line after a header,
/// with a `*` in front of the default and the source's media types after its
/// description (newer ffmpeg only):
/// `Auto-detected sources for pulse:`
/// `  alsa_output.pci-0000_00_1f.3.analog-stereo.monitor [Monitor of Built-in Audio] (none)`
/// `* alsa_input.pci-0000_00_1f.3.analog-stereo [Built-in Audio Analog Stereo] (none)`
pub fn parse_device_sources(output: &str) -> Vec<DeviceSource> {
    output
        .lines()
        .filter_map(|line| {
            let is_default = match line.get(..2)? {
                "* " => true,
                "  " => false,
                _ => return None,
            };
            let (name, rest) = line[2..].split_once(" [")?;
            let (description, _media_types) = rest.rsplit_once(']')?;
            Some(DeviceSource {
                name: name.to_string(),
                description: description.to_string(),
                is_default,
            })
        })
        .collect()
}

/// PulseAudio's name for the monitor of whatever output is the default. Both
/// PulseAudio and PipeWire's pulse server resolve it when the source opens, so
/// the recording follows the output the user is actually listening to.
const PULSE_DEFAULT_MONITOR: &str = "@DEFAULT_MONITOR@";

/// True if `source` is the monitor of a PulseAudio output: what that output
/// plays, rather than an input. Both PulseAudio and PipeWire name the monitor
/// of output `<sink>` `<sink>.monitor`.
fn is_pulse_monitor(source: &DeviceSource) -> bool {
    source.name.ends_with(".monitor")
}

/// The PulseAudio source to record system audio from: the default output's
/// monitor, when the server has any output monitor at all.
pub fn find_pulse_system_audio(sources: &[DeviceSource]) -> Option<String> {
    sources
        .iter()
        .any(is_pulse_monitor)
        .then(|| PULSE_DEFAULT_MONITOR.to_string())
}

/// The PulseAudio source to record the microphone from: the default input,
/// unless the default is an output's monitor, in which case the first input
/// that is not one.
pub fn find_pulse_microphone(sources: &[DeviceSource]) -> Option<String> {
    let inputs: Vec<&DeviceSource> = sources
        .iter()
        .filter(|source| !is_pulse_monitor(source))
        .collect();
    inputs
        .iter()
        .find(|source| source.is_default)
        .or(inputs.first())
        .map(|source| source.name.clone())
}

/// Parse the audio device names out of
/// `ffmpeg -list_devices true -f dshow -i dummy`.
///
/// ffmpeg writes the listing on stderr: one device per line, its name quoted
/// and followed by the media it carries, then its alternative name:
/// `[dshow @ 000001e5d6a8b2c0] "Microphone (Realtek(R) Audio)" (audio)`
/// `[dshow @ 000001e5d6a8b2c0]   Alternative name "@device_cm_{33D9A762-...}\wave_{...}"`
pub fn parse_dshow_audio_devices(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let body = strip_device_log_prefix(line.trim())?;
            let (name, media) = body.strip_prefix('"')?.split_once('"')?;
            media.contains("audio").then(|| name.to_string())
        })
        .collect()
}

/// Names, lowercased, of DirectShow devices that record what the machine plays
/// rather than a microphone: the "Stereo Mix" many sound drivers ship
/// (disabled by default, and named in the Windows display language), and
/// virtual loopback drivers.
const DSHOW_LOOPBACK_NAMES: [&str; 9] = [
    "stereo mix",
    "stereomix",
    "mezcla estéreo",
    "mixage stéréo",
    "mixagem estéreo",
    "missaggio stereo",
    "what u hear",
    "virtual-audio-capturer",
    "cable output",
];

/// True if `name` is a DirectShow loopback device. See [`DSHOW_LOOPBACK_NAMES`].
fn is_dshow_loopback(name: &str) -> bool {
    let lower = name.to_lowercase();
    DSHOW_LOOPBACK_NAMES
        .iter()
        .any(|loopback| lower.contains(loopback))
}

/// The DirectShow device to record system audio from: the first loopback.
pub fn find_dshow_loopback(devices: &[String]) -> Option<String> {
    devices.iter().find(|name| is_dshow_loopback(name)).cloned()
}

/// The DirectShow device to record the microphone from, skipping loopbacks.
/// See [`pick_microphone`].
pub fn find_dshow_microphone(devices: &[String]) -> Option<String> {
    let names: Vec<&str> = devices.iter().map(String::as_str).collect();
    pick_microphone(&names, is_dshow_loopback).map(|position| devices[position].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_written_length_reads_hms_token() {
        assert_eq!(
            parse_written_length("frame= 1 time=00:00:15.000 bitrate=N/A"),
            Some(Duration::from_secs(15))
        );
    }

    #[test]
    fn parse_written_length_reads_minutes_seconds_token() {
        assert_eq!(
            parse_written_length("time=01:30.500 speed=1x"),
            Some(Duration::from_millis(90_500))
        );
    }

    #[test]
    fn parse_written_length_finds_last_of_many_newline_separated_updates() {
        let stderr = "frame=  120 size=  1024kB time=00:00:05.000\n\
                      frame=  240 size=  2048kB time=00:00:10.000\n\
                      frame=  360 size=  3072kB time=00:00:15.000";
        assert_eq!(parse_written_length(stderr), Some(Duration::from_secs(15)));
    }

    /// ffmpeg overwrites its progress line with `\r`, so a single
    /// `\n`-delimited line can carry several `time=` tokens. The last one is
    /// the written range; the first one would under-report it and fire a
    /// spurious short-cut warning.
    #[test]
    fn parse_written_length_finds_last_of_carriage_return_separated_updates() {
        let stderr = "frame=  120 time=00:00:05.00 speed=1x\r\
                      frame=  240 time=00:00:10.00 speed=1x\r\
                      frame=  360 time=00:00:16.50 speed=1x\r\
                      frame=  480 time=00:00:49.43 speed=1x";
        assert_eq!(
            parse_written_length(stderr),
            Some(Duration::from_millis(49_430))
        );
    }

    #[test]
    fn parse_written_length_ignores_trailing_summary_without_progress() {
        let stderr = "frame=  120 time=00:00:05.00 speed=1x\r\
                      frame=  240 time=00:00:12.00 speed=1x\n\
                      [out#0] video:1024kB audio:64kB muxing overhead: 0.4%\n";
        assert_eq!(parse_written_length(stderr), Some(Duration::from_secs(12)));
    }

    #[test]
    fn parse_written_length_without_time_token() {
        assert_eq!(parse_written_length("frame=  120 fps=0.0 q=28.0"), None);
        assert_eq!(parse_written_length(""), None);
    }

    const OUTPUT: &str = "vidcapture_2026-05-28_14-30-00.mp4";

    /// The screen alone, with no audio device.
    fn screen_only(screen: Screen) -> CaptureDevices {
        CaptureDevices {
            screen,
            system_audio: None,
            microphone: None,
        }
    }

    /// A macOS capture with no audio device.
    fn base_config() -> CaptureConfig {
        CaptureConfig::new(
            OUTPUT.to_string(),
            screen_only(Screen::Avfoundation),
        )
    }

    /// A macOS capture with BlackHole 2ch at audio index 0 and the
    /// microphone at index 1.
    fn audio_config() -> CaptureConfig {
        CaptureConfig::new(
            OUTPUT.to_string(),
            CaptureDevices {
                screen: Screen::Avfoundation,
                system_audio: Some("0".to_string()),
                microphone: Some("1".to_string()),
            },
        )
    }

    fn get_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn simple_capture_command() {
        let config = base_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Check input format
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"avfoundation".to_string()));

        // Check video codec
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"libx264".to_string()));

        // Video-only path must NOT request an audio codec or any audio filter.
        assert!(!args.contains(&"-c:a".to_string()));
        assert!(!args.contains(&"aac".to_string()));
        assert!(!args.contains(&"filter_complex".to_string()));

        // Check output file
        assert!(args.contains(&"vidcapture_2026-05-28_14-30-00.mp4".to_string()));

        // Check no duration flag
        assert!(!args.contains(&"-t".to_string()));

        // Check no segment mode
        assert!(!args.contains(&"segment".to_string()));
    }

    #[test]
    fn screen_capture_uses_full_screen_video_input() {
        let args = get_args(&build_capture_command(&base_config()));

        let input_position = args
            .iter()
            .position(|arg| arg == "-i")
            .expect("screen input flag should be present");
        assert_eq!(args[input_position + 1], "1:none");

        let avfoundation_inputs = args
            .windows(2)
            .filter(|window| window[0] == "-f" && window[1] == "avfoundation")
            .count();
        assert_eq!(avfoundation_inputs, 1, "screen-only capture needs one input");
        assert!(
            args.iter().any(|arg| arg.ends_with(".mp4")),
            "screen capture should write an MP4 output"
        );
    }

    #[test]
    fn audio_capture_uses_two_avfoundation_inputs() {
        let config = audio_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Two `-f avfoundation` blocks should be present.
        let avf_count = args
            .iter()
            .enumerate()
            .filter(|(i, a)| *a == "avfoundation" && i.checked_sub(1).and_then(|j| args.get(j)) == Some(&"-f".to_string()))
            .count();
        assert_eq!(
            avf_count, 2,
            "expected two avfoundation inputs, got {:?}",
            args
        );

        // Screen input binds video 1 to audio 0 (BlackHole).
        let screen_idx = args.iter().position(|a| a == "1:0").expect("screen selector 1:0 missing");
        // Mic input is no-video + mic index.
        let mic_idx = args.iter().position(|a| a == ":1").expect("mic selector :1 missing");
        assert!(mic_idx > screen_idx, "mic input must come after screen input");
    }

    #[test]
    fn audio_capture_mixes_streams_with_amix_filter() {
        let config = audio_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let fcp = args
            .iter()
            .position(|a| a == "-filter_complex")
            .expect("-filter_complex missing");
        let filter = &args[fcp + 1];
        assert!(filter.contains("[0:a]"), "filter should reference [0:a], got: {}", filter);
        assert!(filter.contains("[1:a]"), "filter should reference [1:a], got: {}", filter);
        assert!(filter.contains("amix=inputs=2"), "filter should mix both inputs, got: {}", filter);
        assert!(
            filter.contains("[aout]"),
            "filter should label output [aout], got: {}",
            filter
        );
    }

    #[test]
    fn audio_capture_resamples_each_input_for_sync() {
        // Each AVFoundation input has its own clock; without per-input
        // aresample they drift out of sync over time. The spec requires
        // audio stays in sync with video, so the filter chain must include
        // aresample=async=1 on each input.
        let config = audio_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let fcp = args
            .iter()
            .position(|a| a == "-filter_complex")
            .expect("-filter_complex missing");
        let filter = &args[fcp + 1];
        // Two `aresample=async=1` filters — one per input.
        let occurrences = filter.matches("aresample=async=1").count();
        assert_eq!(
            occurrences, 2,
            "expected one aresample=async=1 per audio input, got filter: {}",
            filter
        );
    }

    #[test]
    fn audio_capture_maps_video_and_mixed_audio() {
        let config = audio_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let map_positions: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-map")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(map_positions.len(), 2, "expected exactly two -map flags, got {:?}", args);
        assert_eq!(args[map_positions[0] + 1], "0:v");
        assert_eq!(args[map_positions[1] + 1], "[aout]");
    }

    #[test]
    fn audio_capture_emits_aac_codec() {
        let config = audio_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        assert!(args.contains(&"-c:a".to_string()));
        assert!(args.contains(&"aac".to_string()));
        assert!(args.contains(&"128k".to_string()));
    }

    /// With BlackHole and a microphone, the macOS command is exactly the one
    /// vidcapture has always built: the cross-platform rework must not change
    /// what a working macOS setup records.
    #[test]
    fn macos_capture_with_blackhole_and_microphone_keeps_its_command() {
        let args = get_args(&build_capture_command(&audio_config()));

        assert_eq!(
            args,
            [
                "-y",
                "-f", "avfoundation", "-i", "1:0",
                "-f", "avfoundation", "-i", ":1",
                "-filter_complex",
                "[0:a]aresample=async=1:first_pts=0[a0];\
                 [1:a]aresample=async=1:first_pts=0[a1];\
                 [a0][a1]amix=inputs=2:duration=longest[aout]",
                "-map", "0:v", "-map", "[aout]",
                "-c:a", "aac", "-b:a", "128k",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                OUTPUT,
            ]
        );
    }

    /// Without BlackHole, the microphone takes system audio's place in the
    /// screen input, and is resampled alone rather than mixed.
    #[test]
    fn macos_capture_without_blackhole_records_the_microphone() {
        let config = CaptureConfig::new(
            OUTPUT.to_string(),
            CaptureDevices {
                microphone: Some("2".to_string()),
                ..screen_only(Screen::Avfoundation)
            },
        );
        let args = get_args(&build_capture_command(&config));

        assert_eq!(
            args,
            [
                "-y",
                "-f", "avfoundation", "-i", "1:2",
                "-filter_complex", "[0:a]aresample=async=1:first_pts=0[aout]",
                "-map", "0:v", "-map", "[aout]",
                "-c:a", "aac", "-b:a", "128k",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                OUTPUT,
            ]
        );
    }

    #[test]
    fn linux_capture_grabs_the_x11_display_and_mixes_pulse_sources() {
        let config = CaptureConfig::new(
            OUTPUT.to_string(),
            CaptureDevices {
                screen: Screen::X11 { display: ":1".to_string() },
                system_audio: Some("@DEFAULT_MONITOR@".to_string()),
                microphone: Some("alsa_input.usb-mic".to_string()),
            },
        );
        let args = get_args(&build_capture_command(&config));

        assert_eq!(
            args,
            [
                "-y",
                "-thread_queue_size", "1024", "-f", "x11grab", "-i", ":1",
                "-thread_queue_size", "1024", "-f", "pulse", "-i", "@DEFAULT_MONITOR@",
                "-thread_queue_size", "1024", "-f", "pulse", "-i", "alsa_input.usb-mic",
                "-filter_complex",
                "[1:a]aresample=async=1:first_pts=0[a0];\
                 [2:a]aresample=async=1:first_pts=0[a1];\
                 [a0][a1]amix=inputs=2:duration=longest[aout]",
                "-map", "0:v", "-map", "[aout]",
                "-c:a", "aac", "-b:a", "128k",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                "-pix_fmt", "yuv420p",
                OUTPUT,
            ]
        );
    }

    #[test]
    fn linux_capture_without_audio_grabs_only_the_display() {
        let config = CaptureConfig::new(
            OUTPUT.to_string(),
            screen_only(Screen::X11 { display: ":0".to_string() }),
        );
        let args = get_args(&build_capture_command(&config));

        assert_eq!(
            args,
            [
                "-y",
                "-thread_queue_size", "1024", "-f", "x11grab", "-i", ":0",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                "-pix_fmt", "yuv420p",
                OUTPUT,
            ]
        );
    }

    #[test]
    fn windows_capture_grabs_the_desktop_and_mixes_dshow_devices() {
        let config = CaptureConfig::new(
            OUTPUT.to_string(),
            CaptureDevices {
                screen: Screen::Gdi,
                system_audio: Some("Stereo Mix (Realtek(R) Audio)".to_string()),
                microphone: Some("Microphone (Realtek(R) Audio)".to_string()),
            },
        );
        let args = get_args(&build_capture_command(&config));

        assert_eq!(
            args,
            [
                "-y",
                "-thread_queue_size", "1024", "-f", "gdigrab", "-i", "desktop",
                "-thread_queue_size", "1024", "-f", "dshow",
                "-i", "audio=Stereo Mix (Realtek(R) Audio)",
                "-thread_queue_size", "1024", "-f", "dshow",
                "-i", "audio=Microphone (Realtek(R) Audio)",
                "-filter_complex",
                "[1:a]aresample=async=1:first_pts=0[a0];\
                 [2:a]aresample=async=1:first_pts=0[a1];\
                 [a0][a1]amix=inputs=2:duration=longest[aout]",
                "-map", "0:v", "-map", "[aout]",
                "-c:a", "aac", "-b:a", "128k",
                "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23",
                "-pix_fmt", "yuv420p",
                OUTPUT,
            ]
        );
    }

    #[test]
    fn windows_capture_without_a_loopback_records_the_microphone_alone() {
        let config = CaptureConfig::new(
            OUTPUT.to_string(),
            CaptureDevices {
                microphone: Some("Microphone (USB Audio)".to_string()),
                ..screen_only(Screen::Gdi)
            },
        );
        let args = get_args(&build_capture_command(&config));

        let filter_position = args
            .iter()
            .position(|arg| arg == "-filter_complex")
            .expect("-filter_complex missing");
        assert_eq!(
            args[filter_position + 1],
            "[1:a]aresample=async=1:first_pts=0[aout]"
        );
        assert!(args.contains(&"audio=Microphone (USB Audio)".to_string()));
        assert_eq!(
            args.iter().filter(|arg| *arg == "dshow").count(),
            1,
            "only the microphone should open a dshow input, got {:?}",
            args
        );
    }

    /// Duration and interval are output options, so they apply the same way
    /// whichever device grabbed the screen.
    #[test]
    fn duration_and_interval_apply_on_every_screen() {
        for screen in [
            Screen::Avfoundation,
            Screen::X11 { display: ":0".to_string() },
            Screen::Gdi,
        ] {
            let config = CaptureConfig::new(OUTPUT.to_string(), screen_only(screen))
                .with_duration(Duration::from_secs(60))
                .with_interval(Duration::from_secs(10));
            let args = get_args(&build_capture_command(&config));

            let t_pos = args.iter().position(|a| a == "-t").expect("-t flag not found");
            assert_eq!(args[t_pos + 1], "60.000");
            assert!(args.contains(&"segment".to_string()));
            assert_eq!(args.last().unwrap(), "vidcapture_2026-05-28_14-30-00_seg%03d.mp4");
        }
    }

    #[test]
    fn capture_with_duration() {
        let config = base_config().with_duration(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Find -t flag and its value
        let t_pos = args.iter().position(|a| a == "-t").expect("-t flag not found");
        assert_eq!(args[t_pos + 1], "10.000");
    }

    #[test]
    fn capture_with_fractional_duration() {
        let config = base_config().with_duration(Duration::from_millis(1_500));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let t_pos = args.iter().position(|a| a == "-t").expect("-t flag not found");
        assert_eq!(
            args[t_pos + 1],
            "1.500",
            "1.5s duration must be passed to ffmpeg as fractional seconds, not truncated to 1"
        );
    }

    #[test]
    fn capture_with_interval() {
        let config = base_config().with_interval(Duration::from_secs(30));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Check segment mode
        assert!(args.contains(&"segment".to_string()));

        // Find -segment_time flag
        let st_pos = args
            .iter()
            .position(|a| a == "-segment_time")
            .expect("-segment_time not found");
        assert_eq!(args[st_pos + 1], "30.000");

        // Check segment output pattern exists in args
        let has_segment_pattern = args.iter().any(|a| a.contains("seg%03d"));
        assert!(has_segment_pattern, "Expected segment pattern in args: {:?}", args);
    }

    #[test]
    fn capture_with_duration_and_interval() {
        let config = base_config()
            .with_duration(Duration::from_secs(60))
            .with_interval(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Check duration
        let t_pos = args.iter().position(|a| a == "-t").expect("-t flag not found");
        assert_eq!(args[t_pos + 1], "60.000");

        // Check interval
        let st_pos = args
            .iter()
            .position(|a| a == "-segment_time")
            .expect("-segment_time not found");
        assert_eq!(args[st_pos + 1], "10.000");
    }

    #[test]
    fn segment_pattern_generation() {
        let result = output::segment_ffmpeg_pattern(Path::new("vidcapture_2026-05-28_14-30-00.mp4"));
        assert_eq!(
            result,
            Path::new("vidcapture_2026-05-28_14-30-00_seg%03d.mp4")
        );
    }

    // ---- interval mode (issue #6) requires seamless split ----

    /// Without `-reset_timestamps 1`, segment 002 starts at t=10s instead of
    /// t=0s, which breaks players that expect each segment to begin at the
    /// timeline start. The spec requires a valid, playable MP4 per segment.
    #[test]
    fn capture_with_interval_resets_timestamps_for_each_segment() {
        let config = base_config().with_interval(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let pos = args
            .iter()
            .position(|a| a == "-reset_timestamps")
            .expect("-reset_timestamps flag missing — segments won't play cleanly");
        assert_eq!(
            args[pos + 1], "1",
            "-reset_timestamps must be 1 to give each segment a fresh t=0"
        );
    }

    /// Place keyframes exactly at segment boundaries so the segment muxer can
    /// split on a keyframe, with no gap, no duplicate frame, and no half-GOP
    /// at the cut. The expression must use `n_forced*<interval>` so that
    /// `n_forced` increments after each forced keyframe and the next
    /// boundary is hit at the right time.
    #[test]
    fn capture_with_interval_force_keyframes_align_to_segment_boundary() {
        let config = base_config().with_interval(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let pos = args
            .iter()
            .position(|a| a == "-force_key_frames")
            .expect("-force_key_frames flag missing — segments will not split at keyframes");
        let expr = &args[pos + 1];
        // The exact expression shape matters: `expr:gte(t,n_forced*10)`
        // forces a keyframe at every multiple of 10 seconds. A weaker
        // expression like `expr:gte(t,10)` would force a single keyframe
        // and never another, breaking the seamless-split guarantee.
        assert_eq!(
            expr, "expr:gte(t,n_forced*10.000)",
            "force_key_frames must use n_forced*<interval> for repeated boundaries, got: {}",
            expr
        );
    }

    /// Without interval mode, the seamless-split flags must not be added:
    /// `-reset_timestamps 1` and `-force_key_frames` are only relevant when
    /// the segment muxer is in use.
    #[test]
    fn capture_without_interval_omits_seamless_split_flags() {
        let config = base_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        assert!(
            !args.contains(&"-reset_timestamps".to_string()),
            "non-interval capture must not request segment-only flags"
        );
        assert!(
            !args.contains(&"-force_key_frames".to_string()),
            "non-interval capture must not request segment-only flags"
        );
    }

    /// The segment filename pattern embedded in the ffmpeg command must be
    /// `..._seg%03d.mp4` so the segments ffmpeg actually writes match the
    /// `_segNNN` suffix the issue requires.
    #[test]
    fn capture_with_interval_segment_pattern_uses_three_digit_padding() {
        let config = base_config().with_interval(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        let pattern = args
            .iter()
            .find(|a| a.contains("seg%03d"))
            .expect("expected segment pattern with %03d padding in args");
        assert_eq!(
            pattern, "vidcapture_2026-05-28_14-30-00_seg%03d.mp4",
            "segment pattern must match the issue spec"
        );
    }

    #[test]
    fn segment_pattern_with_directory() {
        let result = output::segment_ffmpeg_pattern(Path::new("/tmp/output/vidcapture_2026-05-28_14-30-00.mp4"));
        assert_eq!(
            result,
            Path::new("/tmp/output/vidcapture_2026-05-28_14-30-00_seg%03d.mp4")
        );
    }

    #[test]
    fn capture_command_overwrite_flag() {
        let config = base_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        assert!(args.contains(&"-y".to_string()));
    }

    // ---- avfoundation listing parser ----

    const SAMPLE_LISTING: &str = "\
ffmpeg version 8.1.1 Copyright (c) 2000-2026 the FFmpeg developers
[AVFoundation indev @ 0xc97014140] AVFoundation video devices:
[AVFoundation indev @ 0xc97014140] [0] FaceTime HD Camera
[AVFoundation indev @ 0xc97014140] [1] Capture screen 0
[AVFoundation indev @ 0xc97014140] AVFoundation audio devices:
[AVFoundation indev @ 0xc97014140] [0] BlackHole 2ch
[AVFoundation indev @ 0xc97014140] [1] MacBook Air Microphone
[AVFoundation indev @ 0xc97014140] [2] Microsoft Teams Audio
[in#0 @ 0xc97014000] Error opening input: Input/output error
";

    #[test]
    fn parse_avfoundation_listing_extracts_video_and_audio() {
        let (video, audio) = parse_avfoundation_listing(SAMPLE_LISTING);

        assert_eq!(
            video,
            vec![
                AvfoundationDevice {
                    index: 0,
                    name: "FaceTime HD Camera".to_string(),
                },
                AvfoundationDevice {
                    index: 1,
                    name: "Capture screen 0".to_string(),
                },
            ]
        );
        assert_eq!(
            audio,
            vec![
                AvfoundationDevice {
                    index: 0,
                    name: "BlackHole 2ch".to_string(),
                },
                AvfoundationDevice {
                    index: 1,
                    name: "MacBook Air Microphone".to_string(),
                },
                AvfoundationDevice {
                    index: 2,
                    name: "Microsoft Teams Audio".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_avfoundation_listing_handles_missing_sections() {
        // Only a video section, no audio section.
        let listing = "\
[AVFoundation indev @ 0xc97014140] AVFoundation video devices:
[AVFoundation indev @ 0xc97014140] [0] FaceTime HD Camera
";
        let (video, audio) = parse_avfoundation_listing(listing);

        assert_eq!(video.len(), 1);
        assert_eq!(audio.len(), 0);
    }

    #[test]
    fn parse_avfoundation_listing_handles_only_audio() {
        let listing = "\
[AVFoundation indev @ 0xc97014140] AVFoundation audio devices:
[AVFoundation indev @ 0xc97014140] [0] BlackHole 2ch
";
        let (video, audio) = parse_avfoundation_listing(listing);

        assert_eq!(video.len(), 0);
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].name, "BlackHole 2ch");
    }

    #[test]
    fn parse_avfoundation_listing_empty_input() {
        let (video, audio) = parse_avfoundation_listing("");
        assert!(video.is_empty());
        assert!(audio.is_empty());
    }

    #[test]
    fn find_blackhole_returns_index_in_listing() {
        let (_, audio) = parse_avfoundation_listing(SAMPLE_LISTING);
        assert_eq!(find_blackhole_index(&audio), Some(0));
    }

    #[test]
    fn find_blackhole_requires_2ch_specifically() {
        // 16-channel variant of BlackHole does NOT satisfy "BlackHole 2ch".
        // Without this constraint, the detector would silently pick a different
        // channel count and the spec's "BlackHole 2ch" requirement is broken.
        let devices = vec![
            AvfoundationDevice {
                index: 2,
                name: "BLACKHOLE 16CH".to_string(),
            },
            AvfoundationDevice {
                index: 5,
                name: "MacBook Pro Microphone".to_string(),
            },
        ];
        assert_eq!(
            find_blackhole_index(&devices),
            None,
            "BlackHole 16ch must not match a 'BlackHole 2ch' request"
        );
    }

    #[test]
    fn find_blackhole_accepts_2ch_case_insensitively() {
        let devices = vec![
            AvfoundationDevice {
                index: 0,
                name: "blackhole 2ch".to_string(),
            },
            AvfoundationDevice {
                index: 3,
                name: "Blackhole 2ch".to_string(),
            },
        ];
        // First match wins.
        assert_eq!(find_blackhole_index(&devices), Some(0));
    }

    #[test]
    fn find_blackhole_missing_returns_none() {
        let devices = vec![AvfoundationDevice {
            index: 0,
            name: "MacBook Air Microphone".to_string(),
        }];
        assert_eq!(find_blackhole_index(&devices), None);
    }

    #[test]
    fn find_microphone_prefers_named_mic_over_virtual_input() {
        // A virtual input (e.g. Microsoft Teams) is listed first, but the
        // actual microphone is named "MacBook Air Microphone" — the picker
        // must return the named mic, not the virtual channel.
        let devices = vec![
            AvfoundationDevice {
                index: 0,
                name: "Microsoft Teams Audio".to_string(),
            },
            AvfoundationDevice {
                index: 1,
                name: "MacBook Air Microphone".to_string(),
            },
        ];
        assert_eq!(find_microphone_index(&devices), Some(1));
    }

    #[test]
    fn find_microphone_skips_blackhole() {
        let (_, audio) = parse_avfoundation_listing(SAMPLE_LISTING);
        assert_eq!(find_microphone_index(&audio), Some(1));
    }

    #[test]
    fn find_microphone_returns_none_if_only_blackhole() {
        let devices = vec![AvfoundationDevice {
            index: 0,
            name: "BlackHole 2ch".to_string(),
        }];
        assert_eq!(find_microphone_index(&devices), None);
    }

    #[test]
    fn find_microphone_falls_back_to_first_non_blackhole() {
        // No "Microphone" in the name; the function should still pick the
        // first non-BlackHole device so hosts with vendor-named inputs work.
        let devices = vec![
            AvfoundationDevice {
                index: 0,
                name: "BlackHole 2ch".to_string(),
            },
            AvfoundationDevice {
                index: 2,
                name: "USB Audio CODEC".to_string(),
            },
        ];
        assert_eq!(find_microphone_index(&devices), Some(2));
    }

    // ---- pulse and dshow listing parsers ----

    /// `ffmpeg -sources pulse` as ffmpeg 6.1 prints it against PulseAudio.
    const SAMPLE_PULSE_SOURCES: &str = "\
Auto-detected sources for pulse:
  alsa_output.pci-0000_00_1f.3.analog-stereo.monitor [Monitor of Built-in Audio Analog Stereo] (none)
* alsa_input.pci-0000_00_1f.3.analog-stereo [Built-in Audio Analog Stereo] (none)
  alsa_input.usb-Blue_Yeti-00.analog-stereo [Yeti Stereo Microphone [Blue]] (none)
";

    #[test]
    fn parse_device_sources_reads_names_descriptions_and_the_default() {
        let sources = parse_device_sources(SAMPLE_PULSE_SOURCES);

        assert_eq!(
            sources,
            vec![
                DeviceSource {
                    name: "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor".to_string(),
                    description: "Monitor of Built-in Audio Analog Stereo".to_string(),
                    is_default: false,
                },
                DeviceSource {
                    name: "alsa_input.pci-0000_00_1f.3.analog-stereo".to_string(),
                    description: "Built-in Audio Analog Stereo".to_string(),
                    is_default: true,
                },
                DeviceSource {
                    name: "alsa_input.usb-Blue_Yeti-00.analog-stereo".to_string(),
                    description: "Yeti Stereo Microphone [Blue]".to_string(),
                    is_default: false,
                },
            ]
        );
    }

    /// Before ffmpeg 5 the media types after the description were not printed.
    #[test]
    fn parse_device_sources_accepts_lines_without_media_types() {
        let sources = parse_device_sources("* alsa_input.mic [Mic]\n");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "alsa_input.mic");
        assert_eq!(sources[0].description, "Mic");
    }

    #[test]
    fn parse_device_sources_without_a_pulse_server_lists_nothing() {
        let listing = "Auto-detected sources for pulse:\n\
                       Cannot list sources: Connection refused\n";
        assert!(parse_device_sources(listing).is_empty());
        assert!(parse_device_sources("").is_empty());
    }

    #[test]
    fn pulse_system_audio_is_the_default_output_monitor() {
        let sources = parse_device_sources(SAMPLE_PULSE_SOURCES);
        assert_eq!(
            find_pulse_system_audio(&sources),
            Some("@DEFAULT_MONITOR@".to_string())
        );
    }

    #[test]
    fn pulse_system_audio_needs_an_output_monitor() {
        let sources = parse_device_sources("* alsa_input.mic [Mic] (none)\n");
        assert_eq!(find_pulse_system_audio(&sources), None);
    }

    #[test]
    fn pulse_microphone_is_the_default_input() {
        let sources = parse_device_sources(SAMPLE_PULSE_SOURCES);
        assert_eq!(
            find_pulse_microphone(&sources),
            Some("alsa_input.pci-0000_00_1f.3.analog-stereo".to_string())
        );
    }

    /// With no input hardware, PulseAudio makes a monitor the default source;
    /// recording it as the microphone would record system audio twice.
    #[test]
    fn pulse_microphone_never_picks_a_monitor_even_when_it_is_the_default() {
        let monitor_default = parse_device_sources(
            "* alsa_output.hdmi.monitor [Monitor of HDMI] (none)\n\
             \x20 alsa_input.usb-mic [USB Mic] (none)\n",
        );
        assert_eq!(
            find_pulse_microphone(&monitor_default),
            Some("alsa_input.usb-mic".to_string())
        );

        let only_monitors =
            parse_device_sources("* alsa_output.hdmi.monitor [Monitor of HDMI] (none)\n");
        assert_eq!(find_pulse_microphone(&only_monitors), None);
    }

    /// `ffmpeg -list_devices true -f dshow -i dummy` on a Windows machine whose
    /// display language is Spanish.
    const SAMPLE_DSHOW_LISTING: &str = r#"[dshow @ 000001e5d6a8b2c0] "Integrated Camera" (video)
[dshow @ 000001e5d6a8b2c0]   Alternative name "@device_pnp_\\?\usb#vid_04f2&pid_b6be&mi_00#6&2b7f2e3&0&0000#{65e8773d-8f56-11d0-a3b9-00a0c90629ac}\global"
[dshow @ 000001e5d6a8b2c0] "Mezcla estéreo (Realtek(R) Audio)" (audio)
[dshow @ 000001e5d6a8b2c0]   Alternative name "@device_cm_{33D9A762-90C8-11D0-BD43-00A0C911CE86}\wave_{1C2D3E4F-5A6B-4C7D-8E9F-0A1B2C3D4E5F}"
[dshow @ 000001e5d6a8b2c0] "Micrófono (Realtek(R) Audio)" (audio)
[dshow @ 000001e5d6a8b2c0]   Alternative name "@device_cm_{33D9A762-90C8-11D0-BD43-00A0C911CE86}\wave_{8F2A5C1E-1B2D-4C3E-9F4A-5B6C7D8E9F01}"
[dshow @ 000001e5d6a8b2c0] "Capture Card" (audio, video)
[in#0 @ 000001e5d6a8a1c0] Error opening input: Immediate exit requested
Error opening input file dummy.
"#;

    #[test]
    fn parse_dshow_audio_devices_lists_every_device_carrying_audio() {
        assert_eq!(
            parse_dshow_audio_devices(SAMPLE_DSHOW_LISTING),
            vec![
                "Mezcla estéreo (Realtek(R) Audio)".to_string(),
                "Micrófono (Realtek(R) Audio)".to_string(),
                "Capture Card".to_string(),
            ]
        );
    }

    #[test]
    fn parse_dshow_audio_devices_empty_input() {
        assert!(parse_dshow_audio_devices("").is_empty());
    }

    #[test]
    fn dshow_loopback_is_recognised_in_the_display_language() {
        let audio = parse_dshow_audio_devices(SAMPLE_DSHOW_LISTING);
        assert_eq!(
            find_dshow_loopback(&audio),
            Some("Mezcla estéreo (Realtek(R) Audio)".to_string())
        );
        assert_eq!(
            find_dshow_loopback(&["Stereo Mix (Realtek High Definition Audio)".to_string()]),
            Some("Stereo Mix (Realtek High Definition Audio)".to_string())
        );
        assert_eq!(
            find_dshow_loopback(&["virtual-audio-capturer".to_string()]),
            Some("virtual-audio-capturer".to_string())
        );
    }

    #[test]
    fn dshow_loopback_missing_returns_none() {
        assert_eq!(
            find_dshow_loopback(&["Microphone (Realtek(R) Audio)".to_string()]),
            None
        );
    }

    /// The Spanish microphone is not named "Microphone", so this exercises the
    /// fallback — which must still skip the loopback listed before it.
    #[test]
    fn dshow_microphone_skips_the_loopback() {
        let audio = parse_dshow_audio_devices(SAMPLE_DSHOW_LISTING);
        assert_eq!(
            find_dshow_microphone(&audio),
            Some("Micrófono (Realtek(R) Audio)".to_string())
        );
    }

    #[test]
    fn dshow_microphone_prefers_a_named_microphone() {
        let audio = vec![
            "Capture Card".to_string(),
            "Microphone Array (Intel® Smart Sound Technology)".to_string(),
        ];
        assert_eq!(
            find_dshow_microphone(&audio),
            Some("Microphone Array (Intel® Smart Sound Technology)".to_string())
        );
    }

    #[test]
    fn dshow_microphone_returns_none_if_only_a_loopback() {
        assert_eq!(find_dshow_microphone(&["Stereo Mix (Realtek(R) Audio)".to_string()]), None);
    }

    // ---- capture warnings ----

    #[test]
    fn missing_audio_warnings_name_each_missing_source() {
        let warnings =
            missing_audio_warnings(&screen_only(Screen::Avfoundation));
        assert_eq!(warnings.len(), 2, "got: {:?}", warnings);
        assert!(warnings[0].starts_with("System audio is not recorded"));
        assert!(warnings[0].contains("BlackHole 2ch"));
        assert!(warnings[1].starts_with("Microphone audio is not recorded"));
    }

    #[test]
    fn missing_audio_warnings_explain_system_audio_per_platform() {
        let linux = missing_audio_warnings(&screen_only(Screen::X11 {
            display: ":0".to_string(),
        }));
        assert!(linux[0].contains("PulseAudio or PipeWire"), "got: {}", linux[0]);

        let windows = missing_audio_warnings(&screen_only(Screen::Gdi));
        assert!(windows[0].contains("Stereo Mix"), "got: {}", windows[0]);
    }

    #[test]
    fn no_missing_audio_warning_when_both_sources_are_present() {
        assert!(missing_audio_warnings(&audio_config().devices).is_empty());
    }

    #[test]
    fn wayland_session_is_recognised_from_xdg_session_type() {
        assert!(is_wayland_session(Some("wayland")));
        assert!(!is_wayland_session(Some("x11")));
        assert!(!is_wayland_session(None));
    }

    #[test]
    fn x11_display_comes_from_display() {
        assert_eq!(x11_display(Some(":1".to_string())).unwrap(), ":1");
    }

    #[test]
    fn x11_display_missing_is_an_error_that_names_display() {
        for display in [None, Some(String::new())] {
            let error = x11_display(display).unwrap_err().to_string();
            assert!(error.contains("DISPLAY"), "got: {}", error);
        }
    }

    /// Live integration test: invokes `ffmpeg -list_devices` on this host and
    /// verifies the parser handles whatever real ffmpeg emits. Skipped when
    /// ffmpeg is not on PATH or BlackHole is not installed.
    #[test]
    #[ignore = "live test against the host's ffmpeg binary"]
    fn detect_avfoundation_devices_live() {
        let result = detect_avfoundation_devices();
        if result.is_err() {
            eprintln!("skipping: ffmpeg not available");
            return;
        }
        let (_, audio) = result.unwrap();
        let bh = find_blackhole_index(&audio);
        eprintln!("blackhole: {:?}", bh);
        eprintln!("audio devices: {:?}", audio);

        assert!(
            bh.is_some(),
            "BlackHole should be present in the host's audio devices; \
             run `ffmpeg -f avfoundation -list_devices true -i \"\"` to verify"
        );
    }

    // ---- run_to_completion ----
    //
    // These drive a POSIX shell as a stand-in for ffmpeg, so they run on Unix
    // only; `cut` and `label` end-to-end tests cover the real thing everywhere.

    /// A one-shot run returns the child's exit status and everything it wrote
    /// to stderr.
    #[test]
    #[cfg(unix)]
    fn run_to_completion_returns_status_and_stderr() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo boom >&2; exit 3"]);

        let (status, stderr) = run_to_completion(cmd, false).unwrap();

        assert_eq!(status.code(), Some(3));
        assert_eq!(stderr, "boom\n");
    }

    /// The child gets no stdin: ffmpeg reads key presses from it (`q` quits),
    /// so a piped or redirected stdin would cut a recording short.
    #[test]
    #[cfg(unix)]
    fn run_to_completion_gives_the_child_no_stdin() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "read line; echo \"read:$line\" >&2"]);

        let (_, stderr) = run_to_completion(cmd, false).unwrap();

        assert_eq!(stderr, "read:\n", "child should see stdin at EOF");
    }

    /// stderr larger than one read buffer must survive intact.
    #[test]
    #[cfg(unix)]
    fn run_to_completion_captures_output_larger_than_the_read_buffer() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "yes ffmpeg | head -c 20000 >&2"]);

        let (status, stderr) = run_to_completion(cmd, false).unwrap();

        assert!(status.success());
        assert_eq!(stderr.len(), 20_000);
    }

    // ---- cut command builder (issue #18) ----

    fn cut_config() -> CutConfig {
        CutConfig::new(
            Path::new("talk.mp4"),
            Duration::from_secs(10),
            Duration::from_secs(15),
            Path::new("talk_cut.mp4"),
        )
    }

    #[test]
    fn cut_default_reencodes_with_h264_and_aac() {
        let cmd = build_cut_command(&cut_config());
        let args = get_args(&cmd);

        // -ss before -i
        let ss_pos = args.iter().position(|a| a == "-ss").expect("-ss missing");
        let i_pos = args.iter().position(|a| a == "-i").expect("-i missing");
        assert!(ss_pos < i_pos, "-ss must come before -i");

        // -t present
        assert!(args.contains(&"-t".to_string()));

        // H.264 video codec
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"ultrafast".to_string()));
        assert!(args.contains(&"23".to_string()));

        // AAC audio codec
        assert!(args.contains(&"-c:a".to_string()));
        assert!(args.contains(&"aac".to_string()));
        assert!(args.contains(&"128k".to_string()));

        // No stream copy
        assert!(!args.contains(&"copy".to_string()));
    }

    #[test]
    fn cut_fast_uses_stream_copy() {
        let config = cut_config().with_fast(true);
        let cmd = build_cut_command(&config);
        let args = get_args(&cmd);

        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"-avoid_negative_ts".to_string()));
        assert!(args.contains(&"make_zero".to_string()));

        // No codec args
        assert!(!args.contains(&"-c:v".to_string()));
        assert!(!args.contains(&"libx264".to_string()));
        assert!(!args.contains(&"-c:a".to_string()));
        assert!(!args.contains(&"aac".to_string()));
    }

    #[test]
    fn cut_time_values_have_three_decimals() {
        let cmd = build_cut_command(&cut_config());
        let args = get_args(&cmd);

        let ss_pos = args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(args[ss_pos + 1], "10.000");

        let t_pos = args.iter().position(|a| a == "-t").unwrap();
        assert_eq!(args[t_pos + 1], "15.000");
    }

    #[test]
    fn cut_subsecond_offset_survives() {
        let config = CutConfig::new(
            Path::new("talk.mp4"),
            Duration::from_millis(1500),
            Duration::from_secs(5),
            Path::new("out.mp4"),
        );
        let cmd = build_cut_command(&config);
        let args = get_args(&cmd);

        let ss_pos = args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(args[ss_pos + 1], "1.500");
    }

    #[test]
    fn cut_overwrite_flag() {
        let cmd = build_cut_command(&cut_config());
        let args = get_args(&cmd);
        assert_eq!(args[0], "-y");
    }

    #[test]
    fn cut_source_and_output_in_command() {
        let cmd = build_cut_command(&cut_config());
        let args = get_args(&cmd);

        let i_pos = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[i_pos + 1], "talk.mp4");

        // Output is the last arg
        assert_eq!(args.last().unwrap(), "talk_cut.mp4");
    }

    /// Live integration test for `detect_capture_devices` on this host. Run it
    /// on each platform to see what a capture session would record.
    #[test]
    #[ignore = "live test against the host's ffmpeg binary"]
    fn detect_capture_devices_live() {
        match detect_capture_devices(Platform::current()) {
            Ok(devices) => {
                eprintln!("detected capture devices: {:?}", devices);
                eprintln!("warnings: {:?}", capture_warnings(&devices));
            }
            Err(e) => {
                eprintln!("skipping: {}", e);
            }
        }
    }

    // ---- label command tests ----

    fn test_label(text: &str) -> Label {
        Label {
            text: text.to_string(),
            start: Duration::from_secs(92),
            length: Duration::from_secs(28),
            position: LabelPosition::Bottom,
            color: "white".to_string(),
            size: 32,
            background: None,
        }
    }

    #[test]
    fn label_command_re_encodes_into_an_mp4_with_a_drawtext_chain() {
        let config = LabelConfig::new(
            Path::new("talk.mp4"),
            vec![test_label("Intro")],
            Path::new("talk_labeled.mp4"),
        );
        let args = get_args(&build_label_command(&config));

        let input = args.iter().position(|a| a == "-i").expect("-i");
        assert_eq!(args[input + 1], "talk.mp4");

        let filter = args.iter().position(|a| a == "-vf").expect("-vf");
        assert!(args[filter + 1].starts_with("drawtext=expansion=none:"));

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"aac".to_string()));
        assert_eq!(args.last().unwrap(), "talk_labeled.mp4");
    }

    #[test]
    fn label_filter_gates_each_label_to_its_own_label_window() {
        let filter = build_label_filter(&[test_label("Intro")], None);
        assert!(
            filter.contains("enable='between(t,92.000,120.000)'"),
            "filter should gate the label to its window, got: {}",
            filter
        );
    }

    #[test]
    fn label_filter_chains_one_drawtext_per_label_in_order() {
        let mut second = test_label("Demo");
        second.start = Duration::from_secs(120);
        let filter = build_label_filter(&[test_label("Intro"), second], None);

        assert_eq!(filter.matches("drawtext=").count(), 2);
        assert!(
            filter.find("text=Intro").unwrap() < filter.find("text=Demo").unwrap(),
            "labels should be drawn in the order given, got: {}",
            filter
        );
    }

    #[test]
    fn label_filter_places_a_top_label_above_a_bottom_one() {
        let mut top = test_label("Intro");
        top.position = LabelPosition::Top;
        let top_filter = build_label_filter(&[top], None);
        let bottom_filter = build_label_filter(&[test_label("Intro")], None);

        assert!(
            top_filter.contains(":y=h*0.05"),
            "a top label should sit a margin below the top edge, got: {}",
            top_filter
        );
        assert!(
            bottom_filter.contains(":y=h-text_h-h*0.05"),
            "a bottom label should sit a margin above the bottom edge, got: {}",
            bottom_filter
        );
        // Both are centred horizontally.
        assert!(top_filter.contains(":x=(w-text_w)/2"));
        assert!(bottom_filter.contains(":x=(w-text_w)/2"));
    }

    #[test]
    fn label_filter_carries_the_color_and_size() {
        let mut label = test_label("Intro");
        label.color = "#ffcc00".to_string();
        label.size = 48;
        let filter = build_label_filter(&[label], None);

        assert!(filter.contains(":fontcolor=#ffcc00"), "got: {}", filter);
        assert!(filter.contains(":fontsize=48"), "got: {}", filter);
    }

    /// A background is drawn as a padded box, so the text sits inside a band
    /// rather than against a tight outline.
    #[test]
    fn label_filter_draws_a_padded_box_for_a_background() {
        let mut label = test_label("Intro");
        label.background = Some("black@0.5".to_string());
        let filter = build_label_filter(&[label], None);

        assert!(filter.contains(":box=1:boxcolor=black@0.5"), "got: {}", filter);
        assert!(filter.contains(":boxborderw=10"), "got: {}", filter);
    }

    /// The padding that keeps the band off the frame edge only exists when
    /// there is a band, so a bare label sits right on the margin.
    #[test]
    fn label_filter_without_a_background_draws_no_box_and_no_padding() {
        let filter = build_label_filter(&[test_label("Intro")], None);

        assert!(!filter.contains("box=1"), "got: {}", filter);
        assert!(filter.contains(":y=h-text_h-h*0.05-0"), "got: {}", filter);
    }

    #[test]
    fn label_filter_uses_the_font_file_when_one_is_given() {
        let filter = build_label_filter(&[test_label("Intro")], Some(Path::new("/fonts/My.ttf")));
        assert!(filter.contains("fontfile=/fonts/My.ttf"), "got: {}", filter);

        let default_filter = build_label_filter(&[test_label("Intro")], None);
        assert!(!default_filter.contains("fontfile="), "got: {}", default_filter);
    }

    /// Every count here was verified by rendering the character with ffmpeg
    /// and reading it back off the frame; they are not interchangeable.
    #[test]
    fn escape_filter_value_table() {
        let cases: &[(&str, &str)] = &[
            ("plain text", "plain text"),
            ("a,b", r"a\,b"),
            ("a;b", r"a\;b"),
            ("[x]", r"\[x\]"),
            ("a:b", r"a\\:b"),
            ("it's", r"it\\\'s"),
            // `expansion=none` makes `%` an ordinary character.
            ("50%", "50%"),
            (r"a\b", r"a\\\\b"),
            ("a=b", "a=b"),
            ("Introducción — ¿listo?", "Introducción — ¿listo?"),
        ];
        for (input, expected) in cases {
            assert_eq!(&escape_filter_value(input), expected, "input: '{}'", input);
        }
    }

    /// A label's text is literal. Without `expansion=none` drawtext would read
    /// `%{pts}` in it as an ffmpeg expression and draw a timestamp instead.
    #[test]
    fn label_filter_turns_off_drawtext_text_expansion() {
        let filter = build_label_filter(&[test_label("50% done %{pts}")], None);

        assert!(filter.starts_with("drawtext=expansion=none:"), "got: {}", filter);
        assert!(filter.contains("text=50% done %{pts}"), "got: {}", filter);
    }

    /// A label's text reaches drawtext as one option value, so nothing in it
    /// can be read as another option or another filter.
    #[test]
    fn label_filter_escapes_text_that_looks_like_filter_syntax() {
        let filter = build_label_filter(&[test_label("x:y,drawtext=text=pwned")], None);

        // The comma and colon come back escaped, so the text stays one option
        // value instead of opening a second filter.
        assert!(filter.contains(r"x\\:y\,drawtext=text=pwned"), "got: {}", filter);
        assert!(
            filter.ends_with("enable='between(t,92.000,120.000)'"),
            "the enable gate must still terminate the filter, got: {}",
            filter
        );
    }
}
