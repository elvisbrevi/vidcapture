use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};

/// Base file name (without extension) used for every capture session.
const BASE_NAME: &str = "vidcapture";

#[derive(Debug, Clone, PartialEq)]
pub struct OutputConfig {
    pub directory: PathBuf,
    pub base_name: String,
}

impl OutputConfig {
    pub fn new(directory: PathBuf, base_name: String) -> Self {
        Self {
            directory,
            base_name,
        }
    }
}

/// Prepare the final output path for a capture session.
///
/// Resolves the output directory (creating it when `-o` is set and the
/// directory does not yet exist), generates the timestamped filename, and
/// returns a non-colliding path. When `requested_dir` is `None`, the current
/// working directory is used without attempting to create it.
pub fn prepare_output_path(
    requested_dir: Option<&Path>,
    timestamp: &DateTime<Local>,
) -> anyhow::Result<PathBuf> {
    let dir = match requested_dir {
        Some(path) => resolve_output_directory(path)?,
        None => std::env::current_dir()?,
    };

    let config = OutputConfig::new(dir, BASE_NAME.to_string());
    let path = generate_filename(&config, timestamp);
    Ok(avoid_collision(&path))
}

/// Generate a timestamped filename for a single-segment capture session.
/// Format: `{base_name}_{YYYY-MM-DD_HH-MM-SS}.mp4`
pub fn generate_filename(config: &OutputConfig, timestamp: &DateTime<Local>) -> PathBuf {
    let formatted = timestamp.format("%Y-%m-%d_%H-%M-%S").to_string();
    let filename = format!("{}_{}.mp4", config.base_name, formatted);
    config.directory.join(filename)
}

/// Generate a segment filename pattern for interval mode.
/// Format: `{base_name}_{YYYY-MM-DD_HH-MM-SS}_seg{NNN}.mp4`
pub fn generate_segment_filename(
    config: &OutputConfig,
    timestamp: &DateTime<Local>,
    segment_number: u32,
) -> PathBuf {
    let formatted = timestamp.format("%Y-%m-%d_%H-%M-%S").to_string();
    let filename = format!(
        "{}_{}_seg{:03}.mp4",
        config.base_name, formatted, segment_number
    );
    config.directory.join(filename)
}

/// Build the ffmpeg segment-muxer output pattern from a base filename.
///
/// ffmpeg's `-f segment` muxer substitutes a printf placeholder at runtime
/// with the 0-padded segment number, so we hand it a path whose filename
/// ends with `_seg%03d.<ext>`. For example, given
/// `vidcapture_2026-05-28_14-30-00.mp4`, this returns
/// `vidcapture_2026-05-28_14-30-00_seg%03d.mp4`.
pub fn segment_ffmpeg_pattern(base_path: &Path) -> PathBuf {
    let parent = base_path.parent().unwrap_or(Path::new("."));
    let stem = base_path.file_stem().unwrap().to_string_lossy();
    let extension = base_path.extension().map(|e| e.to_string_lossy().to_string());

    let segment_name = match extension {
        Some(ext) => format!("{}_seg%03d.{}", stem, ext),
        None => format!("{}_seg%03d", stem),
    };

    parent.join(segment_name)
}

/// Build a user-friendly display path for a segment-mode capture session.
///
/// The result is intent-preserving — the user sees a single string that
/// describes the family of files produced, with a shell glob in place of
/// the printf placeholder. Given
/// `vidcapture_2026-05-28_14-30-00.mp4`, this returns
/// `vidcapture_2026-05-28_14-30-00_seg*.mp4`.
pub fn segment_display_pattern(base_path: &Path) -> PathBuf {
    let parent = base_path.parent().unwrap_or(Path::new("."));
    let stem = base_path.file_stem().unwrap().to_string_lossy();
    let extension = base_path.extension().map(|e| e.to_string_lossy().to_string());

    let segment_name = match extension {
        Some(ext) => format!("{}_seg*.{}", stem, ext),
        None => format!("{}_seg*", stem),
    };

    parent.join(segment_name)
}

/// Resolve the output directory, creating it if it doesn't exist.
/// Returns an error if the path exists but is not a directory, or if creation
/// of a missing path fails (e.g. an intermediate path component is an
/// existing file). All errors include the requested path so the user knows
/// what `-o` value is unusable.
pub fn resolve_output_directory(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        if path.is_dir() {
            return Ok(path.to_path_buf());
        } else {
            anyhow::bail!("Output path exists but is not a directory: {}", path.display());
        }
    }

    std::fs::create_dir_all(path).map_err(|e| {
        anyhow::anyhow!("Failed to create output directory '{}': {}", path.display(), e)
    })?;
    Ok(path.to_path_buf())
}

/// Find a non-colliding filename by appending _1, _2, etc. if the file exists.
pub fn avoid_collision(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().unwrap().to_string_lossy();
    let extension = path.extension().map(|e| e.to_string_lossy().to_string());

    for i in 1.. {
        let new_name = match &extension {
            Some(ext) => format!("{}_{}.{}", stem, i, ext),
            None => format!("{}_{}", stem, i),
        };
        let new_path = parent.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
    }

    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_timestamp() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 5, 28, 14, 30, 0).unwrap()
    }

    fn test_config() -> OutputConfig {
        OutputConfig::new(PathBuf::from("/tmp/vidcapture"), BASE_NAME.to_string())
    }

    #[test]
    fn single_segment_filename_format() {
        let config = test_config();
        let ts = fixed_timestamp();
        let result = generate_filename(&config, &ts);

        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00.mp4")
        );
    }

    #[test]
    fn segment_filename_format() {
        let config = test_config();
        let ts = fixed_timestamp();
        let result = generate_segment_filename(&config, &ts, 1);

        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00_seg001.mp4")
        );
    }

    #[test]
    fn segment_filename_padded_number() {
        let config = test_config();
        let ts = fixed_timestamp();
        let result = generate_segment_filename(&config, &ts, 42);

        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00_seg042.mp4")
        );
    }

    #[test]
    fn segment_ffmpeg_pattern_uses_three_digit_padding() {
        let base = PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00.mp4");
        let result = segment_ffmpeg_pattern(&base);
        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00_seg%03d.mp4")
        );
    }

    #[test]
    fn segment_ffmpeg_pattern_handles_no_extension() {
        let base = PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00");
        let result = segment_ffmpeg_pattern(&base);
        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00_seg%03d")
        );
    }

    #[test]
    fn segment_display_pattern_uses_glob() {
        // The display path is what the user sees in the "Saved to" message.
        // It must use a shell-glob (not a printf placeholder) so the user
        // can copy it into a shell without confusion.
        let base = PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00.mp4");
        let result = segment_display_pattern(&base);
        assert_eq!(
            result,
            PathBuf::from("/tmp/vidcapture/vidcapture_2026-05-28_14-30-00_seg*.mp4")
        );
    }

    #[test]
    fn resolve_existing_directory() {
        let dir = std::env::temp_dir();
        let result = resolve_output_directory(&dir).unwrap();
        assert_eq!(result, dir);
    }

    #[test]
    fn resolve_creates_missing_directory() {
        let dir = std::env::temp_dir().join("vidcapture_test_resolve");
        let _ = std::fs::remove_dir_all(&dir);

        let result = resolve_output_directory(&dir).unwrap();
        assert!(result.exists());
        assert!(result.is_dir());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_rejects_file_path() {
        let file = std::env::temp_dir().join("vidcapture_test_file.txt");
        std::fs::write(&file, "test").unwrap();

        let result = resolve_output_directory(&file);
        assert!(result.is_err());

        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn no_collision_returns_original_path() {
        let path = std::env::temp_dir().join("vidcapture_no_collision.mp4");
        let _ = std::fs::remove_file(&path);

        let result = avoid_collision(&path);
        assert_eq!(result, path);
    }

    #[test]
    fn collision_appends_increment() {
        let dir = std::env::temp_dir().join("vidcapture_collision_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let original = dir.join("test.mp4");
        std::fs::write(&original, "").unwrap();

        let result = avoid_collision(&original);
        assert_eq!(result, dir.join("test_1.mp4"));

        std::fs::write(&result, "").unwrap();
        let result2 = avoid_collision(&original);
        assert_eq!(result2, dir.join("test_2.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- prepare_output_path (issue #5 acceptance criteria) ----

    /// The default-directory path (no `-o`) must place the file in the current
    /// working directory and never create extra directories.
    #[test]
    fn prepare_output_path_uses_cwd_by_default() {
        let ts = fixed_timestamp();
        let path = prepare_output_path(None, &ts).unwrap();

        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            path.parent().unwrap(),
            cwd,
            "default output should land in the current working directory"
        );
        assert!(
            path.file_name().unwrap().to_string_lossy().starts_with("vidcapture_"),
            "filename should still use the vidcapture base name, got: {}",
            path.display()
        );
        assert!(
            path.extension().map(|e| e == "mp4").unwrap_or(false),
            "output should be an .mp4 file, got: {}",
            path.display()
        );
    }

    /// `vidcapture start -o ./recordings/` must save into `./recordings/` and
    /// create the directory if it does not yet exist.
    #[test]
    fn prepare_output_path_creates_missing_directory() {
        let dir = std::env::temp_dir().join("vidcapture_test_prepare_creates");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            !dir.exists(),
            "test precondition: directory should not exist yet"
        );

        let ts = fixed_timestamp();
        let path = prepare_output_path(Some(&dir), &ts).unwrap();

        assert!(dir.exists(), "directory should be created on demand");
        assert!(dir.is_dir(), "created path should be a directory");
        assert_eq!(
            path.parent().unwrap(),
            dir,
            "output path should land inside the requested directory"
        );
        assert!(!path.exists(), "output file should not pre-exist");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Auto-increment: if `vidcapture_2026-05-28_14-30-00.mp4` already exists
    /// in the output directory, the next path should be `..._1.mp4`, then
    /// `..._2.mp4`, and so on.
    #[test]
    fn prepare_output_path_auto_increments_on_collision() {
        let dir = std::env::temp_dir().join("vidcapture_test_prepare_collision");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let ts = fixed_timestamp();

        // First call → base filename.
        let p1 = prepare_output_path(Some(&dir), &ts).unwrap();
        std::fs::write(&p1, "").unwrap();

        // Second call → _1 suffix.
        let p2 = prepare_output_path(Some(&dir), &ts).unwrap();
        assert_ne!(p1, p2, "second path must differ from the first");
        assert_eq!(
            p2,
            dir.join(format!(
                "vidcapture_{}_1.mp4",
                ts.format("%Y-%m-%d_%H-%M-%S")
            )),
            "second path should append _1, got: {}",
            p2.display()
        );
        std::fs::write(&p2, "").unwrap();

        // Third call → _2 suffix.
        let p3 = prepare_output_path(Some(&dir), &ts).unwrap();
        assert_eq!(
            p3,
            dir.join(format!(
                "vidcapture_{}_2.mp4",
                ts.format("%Y-%m-%d_%H-%M-%S")
            )),
            "third path should append _2, got: {}",
            p3.display()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Invalid paths must surface a clear error rather than panicking or
    /// silently falling back.
    #[test]
    fn prepare_output_path_rejects_existing_file_with_clear_message() {
        let file = std::env::temp_dir().join("vidcapture_test_prepare_not_a_dir.txt");
        std::fs::write(&file, "").unwrap();

        let ts = fixed_timestamp();
        let err = prepare_output_path(Some(&file), &ts).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not a directory"),
            "error must explain why the path is unusable, got: {}",
            msg
        );
        assert!(
            msg.contains(file.to_str().unwrap()),
            "error must name the offending path so the user can fix it, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&file);
    }

    /// Nested directories should be created with their full path, matching the
    /// common `vidcapture start -o ./recordings/2026-05-28/` workflow.
    #[test]
    fn prepare_output_path_creates_nested_missing_directories() {
        let base = std::env::temp_dir().join("vidcapture_test_prepare_nested");
        let dir = base.join("a").join("b").join("c");
        let _ = std::fs::remove_dir_all(&base);

        let ts = fixed_timestamp();
        let path = prepare_output_path(Some(&dir), &ts).unwrap();

        assert!(dir.is_dir(), "nested directory tree should be created");
        assert_eq!(path.parent().unwrap(), dir);

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Relative paths like `./recordings/` should resolve relative to the
    /// current working directory. We test by changing the process cwd into a
    /// temp dir and passing a relative name; the resulting directory must
    /// be created under the new cwd (not the original cwd), proving that
    /// the relative name is interpreted by the OS rather than by us.
    #[test]
    fn prepare_output_path_resolves_relative_paths_against_cwd() {
        let dir = std::env::temp_dir().join("vidcapture_test_relative_cwd");
        let _ = std::fs::remove_dir_all(&dir);

        // Build the directory and chdir into it so a relative name resolves
        // there. Save the original cwd so we can restore it even if the
        // assertions fail.
        std::fs::create_dir_all(&dir).unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).expect("chdir into temp dir");

        let result = std::panic::catch_unwind(|| {
            let ts = fixed_timestamp();
            // A bare relative name like "subdir" must be created under cwd.
            let path = prepare_output_path(Some(Path::new("subdir")), &ts).unwrap();
            // The resulting path's parent must equal "subdir" relative to
            // the new cwd — both expressed as relative paths.
            assert_eq!(
                path.parent().unwrap(),
                Path::new("subdir"),
                "relative -o should resolve under cwd, got: {}",
                path.display()
            );
            // And the directory itself must exist under the new cwd.
            assert!(
                dir.join("subdir").is_dir(),
                "subdir should have been created under cwd"
            );
        });

        // Restore cwd before any cleanup so a failed assertion does not
        // corrupt subsequent tests.
        std::env::set_current_dir(&original_cwd).expect("restore cwd");
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    /// When `create_dir_all` fails (e.g. an intermediate component is a
    /// regular file), the error must still name the offending path so the
    /// user can fix their `-o` value.
    #[test]
    fn resolve_output_directory_reports_create_failures_with_path() {
        let blocker = std::env::temp_dir().join("vidcapture_test_blocker.txt");
        std::fs::write(&blocker, "").unwrap();

        // Trying to create a directory at blocker/child must fail because
        // the intermediate component is a file.
        let target = blocker.join("child");
        let err = resolve_output_directory(&target).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(target.to_str().unwrap()),
            "create_dir_all error must include the requested path so the user can identify it, got: {}",
            msg
        );
        assert!(
            msg.to_lowercase().contains("failed")
                || msg.to_lowercase().contains("create")
                || msg.to_lowercase().contains("not a directory"),
            "error should describe the failure mode, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&blocker);
    }
}
