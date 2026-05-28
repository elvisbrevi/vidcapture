use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Configuration for an ffmpeg capture session.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub output_path: String,
    pub duration: Option<Duration>,
    pub interval: Option<Duration>,
    pub verbose: bool,
}

impl CaptureConfig {
    pub fn new(output_path: String) -> Self {
        Self {
            output_path,
            duration: None,
            interval: None,
            verbose: false,
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

/// Build an ffmpeg command for screen + audio capture.
///
/// Uses avfoundation for screen capture and BlackHole 2ch for system audio.
/// Output is MP4 with H.264 video and AAC audio.
pub fn build_capture_command(config: &CaptureConfig) -> Command {
    let mut cmd = Command::new("ffmpeg");

    // Global options (must be before inputs)
    cmd.args(["-y"]);

    // Input: screen capture via avfoundation
    // Format: avfoundation expects "video:audio" device indices
    // "1:" = screen (index 1), "none" for no audio input device
    cmd.args(["-f", "avfoundation", "-i", "1:none"]);

    // Audio: BlackHole 2ch for system audio
    cmd.args(["-f", "avfoundation", "-i", ":0"]);

    // Video codec: H.264
    cmd.args(["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"]);

    // Audio codec: AAC
    cmd.args(["-c:a", "aac", "-b:a", "128k"]);

    // Duration limit
    if let Some(duration) = config.duration {
        cmd.args(["-t", &duration.as_secs().to_string()]);
    }

    // Interval mode: use segment muxer
    if let Some(interval) = config.interval {
        cmd.args(["-f", "segment", "-segment_time", &interval.as_secs().to_string()]);

        // For segment mode, output path needs %03d for segment numbering
        let segment_path = generate_segment_pattern(&config.output_path);
        cmd.arg(&segment_path);
    } else {
        // Single output file
        cmd.arg(&config.output_path);
    }

    cmd
}

/// Generate a segment pattern from an output path.
/// Converts "vidcapture_2026-05-28_14-30-00.mp4" to
/// "vidcapture_2026-05-28_14-30-00_seg%03d.mp4"
fn generate_segment_pattern(output_path: &str) -> String {
    let path = Path::new(output_path);
    let stem = path.file_stem().unwrap().to_string_lossy();
    let extension = path.extension().map(|e| e.to_string_lossy().to_string());
    let parent = path.parent().unwrap_or(Path::new("."));

    let segment_name = match extension {
        Some(ext) => format!("{}_seg%03d.{}", stem, ext),
        None => format!("{}_seg%03d", stem),
    };

    parent.join(segment_name).to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> CaptureConfig {
        CaptureConfig::new("vidcapture_2026-05-28_14-30-00.mp4".to_string())
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

        // Check audio codec
        assert!(args.contains(&"-c:a".to_string()));
        assert!(args.contains(&"aac".to_string()));

        // Check output file
        assert!(args.contains(&"vidcapture_2026-05-28_14-30-00.mp4".to_string()));

        // Check no duration flag
        assert!(!args.contains(&"-t".to_string()));

        // Check no segment mode
        assert!(!args.contains(&"segment".to_string()));
    }

    #[test]
    fn capture_with_duration() {
        let config = base_config().with_duration(Duration::from_secs(10));
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        // Find -t flag and its value
        let t_pos = args.iter().position(|a| a == "-t").expect("-t flag not found");
        assert_eq!(args[t_pos + 1], "10");
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
        assert_eq!(args[st_pos + 1], "30");

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
        assert_eq!(args[t_pos + 1], "60");

        // Check interval
        let st_pos = args
            .iter()
            .position(|a| a == "-segment_time")
            .expect("-segment_time not found");
        assert_eq!(args[st_pos + 1], "10");
    }

    #[test]
    fn segment_pattern_generation() {
        let result = generate_segment_pattern("vidcapture_2026-05-28_14-30-00.mp4");
        assert_eq!(result, "vidcapture_2026-05-28_14-30-00_seg%03d.mp4");
    }

    #[test]
    fn segment_pattern_with_directory() {
        let result = generate_segment_pattern("/tmp/output/vidcapture_2026-05-28_14-30-00.mp4");
        assert_eq!(
            result,
            "/tmp/output/vidcapture_2026-05-28_14-30-00_seg%03d.mp4"
        );
    }

    #[test]
    fn capture_command_overwrite_flag() {
        let config = base_config();
        let cmd = build_capture_command(&config);
        let args = get_args(&cmd);

        assert!(args.contains(&"-y".to_string()));
    }
}
