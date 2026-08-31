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

/// Resolve the output path for a cut operation.
///
/// With no `-o`, the cut lands beside the source as `<source-stem>_cut.mp4`.
/// See [`resolve_derived_output_path`] for the full rules.
pub fn resolve_cut_output_path(source: &Path, output: Option<&Path>) -> anyhow::Result<PathBuf> {
    resolve_derived_output_path(source, output, "cut")
}

/// Resolve the output path for a label pass.
///
/// With no `-o`, the labeled video lands beside the source as
/// `<source-stem>_labeled.mp4`. See [`resolve_derived_output_path`] for the
/// full rules.
pub fn resolve_label_output_path(source: &Path, output: Option<&Path>) -> anyhow::Result<PathBuf> {
    resolve_derived_output_path(source, output, "labeled")
}

/// Resolve the output path for a command that derives a new video from a
/// source video, naming it `<source-stem>_<suffix>.mp4` by default.
///
/// Rules:
/// - No output: `<source-stem>_<suffix>.mp4` beside the source.
/// - Output is a directory (exists+is_dir or ends with separator): put
///   `<source-stem>_<suffix>.mp4` inside it, creating the dir if missing.
/// - Output is a file path: use it directly; parent must exist.
/// - Always `.mp4` regardless of source container.
/// - Refuse to overwrite the source video.
/// - Apply `avoid_collision` to prevent overwriting existing files.
fn resolve_derived_output_path(
    source: &Path,
    output: Option<&Path>,
    suffix: &str,
) -> anyhow::Result<PathBuf> {
    let source_stem = source
        .file_stem()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine source file name: {}", source.display()))?
        .to_string_lossy();

    let resolved = match output {
        None => {
            let dir = source.parent().unwrap_or(Path::new("."));
            dir.join(format!("{}_{}.mp4", source_stem, suffix))
        }
        Some(path) => {
            let is_dir =
                (path.exists() && path.is_dir()) || path.to_string_lossy().ends_with('/');
            if is_dir {
                let dir = resolve_output_directory(path)?;
                dir.join(format!("{}_{}.mp4", source_stem, suffix))
            } else {
                let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
                if let Some(parent) = parent
                    && !parent.exists()
                {
                    anyhow::bail!(
                        "Output path parent directory does not exist: {}",
                        path.display()
                    );
                }
                let mut p = path.to_path_buf();
                p.set_extension("mp4");
                p
            }
        }
    };

    let canonical_source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let canonical_output = resolved
        .canonicalize()
        .unwrap_or_else(|_| resolved.to_path_buf());
    if canonical_source == canonical_output {
        anyhow::bail!(
            "Refusing to overwrite source video: {}",
            source.display()
        );
    }

    Ok(avoid_collision(&resolved))
}

/// Delete a partially written output so a failed run leaves no corrupt file.
pub fn remove_partial_output(path: &Path) {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
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

    /// The process-wide current directory is shared by every test thread, so
    /// the tests that read or change it must not run concurrently.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Take [`CWD_LOCK`], ignoring poisoning: a test that panicked while
    /// holding it still restored the cwd on its way out, so the lock guards
    /// nothing but ordering.
    fn lock_cwd() -> std::sync::MutexGuard<'static, ()> {
        CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The default-directory path (no `-o`) must place the file in the current
    /// working directory and never create extra directories.
    #[test]
    fn prepare_output_path_uses_cwd_by_default() {
        let _guard = lock_cwd();
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
        let _guard = lock_cwd();
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


    // ---- resolve_label_output_path ----

    #[test]
    fn label_no_output_resolves_to_source_stem_labeled_mp4() {
        let source = PathBuf::from("/videos/talk.mp4");
        let result = resolve_label_output_path(&source, None).unwrap();
        assert_eq!(result, PathBuf::from("/videos/talk_labeled.mp4"));
    }

    #[test]
    fn label_output_directory_resolves_inside() {
        let dir = std::env::temp_dir().join("vidcapture_label_dir_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.parent().unwrap().join("talk.mp4");
        let result = resolve_label_output_path(&source, Some(&dir)).unwrap();
        assert_eq!(
            result,
            dir.join("talk_labeled.mp4"),
            "should place talk_labeled.mp4 inside the output directory"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn label_output_file_keeps_the_given_name_as_mp4() {
        let result =
            resolve_label_output_path(Path::new("/videos/talk.mov"), Some(Path::new("/tmp/out.mkv")))
                .unwrap();
        assert_eq!(result, PathBuf::from("/tmp/out.mp4"));
    }

    #[test]
    fn label_refuses_to_overwrite_the_source_video() {
        let dir = std::env::temp_dir().join("vidcapture_label_overwrite_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        std::fs::write(&source, b"video").unwrap();

        let err = resolve_label_output_path(&source, Some(&source)).unwrap_err();
        assert!(
            err.to_string().contains("Refusing to overwrite source"),
            "should refuse to write over the source, got: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A labeled video is a new file every time — a second pass over the same
    /// source must not overwrite the first one's result.
    #[test]
    fn label_output_auto_increments_on_collision() {
        let dir = std::env::temp_dir().join("vidcapture_label_collision_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        std::fs::write(&source, b"video").unwrap();
        std::fs::write(dir.join("talk_labeled.mp4"), b"first").unwrap();

        let result = resolve_label_output_path(&source, None).unwrap();
        assert_eq!(result, dir.join("talk_labeled_1.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- remove_partial_output ----

    #[test]
    fn remove_partial_output_deletes_the_file_and_tolerates_a_missing_one() {
        let dir = std::env::temp_dir().join("vidcapture_partial_output_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let partial = dir.join("half_written.mp4");
        std::fs::write(&partial, b"partial").unwrap();
        remove_partial_output(&partial);
        assert!(!partial.exists(), "a partial output should be deleted");

        // A run that failed before ffmpeg wrote anything leaves nothing behind.
        remove_partial_output(&partial);

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- resolve_cut_output_path (issue #19 acceptance criteria) ----

    #[test]
    fn cut_no_output_resolves_to_source_stem_cut_mp4() {
        let source = PathBuf::from("/videos/talk.mp4");
        let result = resolve_cut_output_path(&source, None).unwrap();
        assert_eq!(result, PathBuf::from("/videos/talk_cut.mp4"));
    }

    #[test]
    fn cut_output_directory_resolves_inside() {
        let dir = std::env::temp_dir().join("vidcapture_cut_dir_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.parent().unwrap().join("talk.mp4");
        let result = resolve_cut_output_path(&source, Some(&dir)).unwrap();
        assert_eq!(
            result,
            dir.join("talk_cut.mp4"),
            "should place talk_cut.mp4 inside the output directory"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_directory_with_trailing_slash_resolves_as_dir() {
        let dir = std::env::temp_dir().join("vidcapture_cut_slash_test");
        let _ = std::fs::remove_dir_all(&dir);
        // Don't create it — the trailing slash should trigger directory creation.
        let mut path_with_slash = dir.to_path_buf();
        path_with_slash.push("");

        let source = dir.parent().unwrap().join("talk.mp4");
        let result = resolve_cut_output_path(&source, Some(&path_with_slash)).unwrap();
        assert!(dir.is_dir(), "directory should be created from trailing slash");
        assert_eq!(result, dir.join("talk_cut.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_missing_dir_with_trailing_slash_creates() {
        let dir = std::env::temp_dir().join("vidcapture_cut_create_test");
        let _ = std::fs::remove_dir_all(&dir);
        let mut path_with_slash = dir.to_path_buf();
        path_with_slash.push("");

        let source = dir.parent().unwrap().join("talk.mp4");
        let result = resolve_cut_output_path(&source, Some(&path_with_slash)).unwrap();
        assert!(dir.is_dir(), "missing output directory should be created from trailing slash");
        assert_eq!(result, dir.join("talk_cut.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_file_path_used_directly() {
        let dir = std::env::temp_dir().join("vidcapture_cut_file_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        let output = dir.join("clip.mp4");
        let result = resolve_cut_output_path(&source, Some(&output)).unwrap();
        assert_eq!(result, output, "should use the exact file path provided");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_refuses_to_overwrite_source() {
        let dir = std::env::temp_dir().join("vidcapture_cut_overwrite_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        std::fs::write(&source, "").unwrap();

        let err = resolve_cut_output_path(&source, Some(&source)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Refusing to overwrite source video"),
            "error must refuse source overwrite, got: {}",
            msg
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_force_mp4_extension() {
        let dir = std::env::temp_dir().join("vidcapture_cut_ext_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mov");
        let output = dir.join("clip.mp4");
        let result = resolve_cut_output_path(&source, Some(&output)).unwrap();
        assert!(
            result.extension().map(|e| e == "mp4").unwrap_or(false),
            "output must be .mp4 regardless of source, got: {}",
            result.display()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_no_flag_yields_mp4_even_for_mov_source() {
        let source = PathBuf::from("/videos/talk.mov");
        let result = resolve_cut_output_path(&source, None).unwrap();
        assert!(
            result.extension().map(|e| e == "mp4").unwrap_or(false),
            "default output must be .mp4 for .mov source, got: {}",
            result.display()
        );
        assert_eq!(result, PathBuf::from("/videos/talk_cut.mp4"));
    }

    #[test]
    fn cut_output_auto_increments_on_collision() {
        let dir = std::env::temp_dir().join("vidcapture_cut_collision_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        std::fs::write(&source, "").unwrap();

        let p1 = resolve_cut_output_path(&source, None).unwrap();
        assert_eq!(p1, dir.join("talk_cut.mp4"));
        std::fs::write(&p1, "").unwrap();

        let p2 = resolve_cut_output_path(&source, None).unwrap();
        assert_eq!(p2, dir.join("talk_cut_1.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cut_output_parent_not_exists_errors() {
        let source = PathBuf::from("/videos/talk.mp4");
        let output = PathBuf::from("/nonexistent_dir/clip.mp4");
        let err = resolve_cut_output_path(&source, Some(&output)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("parent directory does not exist"),
            "error must report missing parent, got: {}",
            msg
        );
        assert!(
            msg.contains(output.to_str().unwrap()),
            "error must name the requested path, got: {}",
            msg
        );
    }

    #[test]
    fn cut_output_existing_file_treated_as_file_not_dir() {
        let dir = std::env::temp_dir().join("vidcapture_cut_file_as_dir_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let source = dir.join("talk.mp4");
        let existing_file = dir.join("existing_file.mp4");
        std::fs::write(&existing_file, "").unwrap();

        // Pass the existing file as output — it should be treated as a file
        // path, not a directory. Since it already exists, avoid_collision
        // renames it to _1.
        let result = resolve_cut_output_path(&source, Some(&existing_file)).unwrap();
        assert_eq!(result, dir.join("existing_file_1.mp4"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
