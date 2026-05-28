use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};

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

    pub fn with_default_directory(base_name: String) -> Self {
        Self {
            directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            base_name,
        }
    }
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

/// Resolve the output directory, creating it if it doesn't exist.
/// Returns an error if the path exists but is not a directory.
pub fn resolve_output_directory(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        if path.is_dir() {
            return Ok(path.to_path_buf());
        } else {
            anyhow::bail!("Output path exists but is not a directory: {}", path.display());
        }
    }

    std::fs::create_dir_all(path)?;
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
        OutputConfig::new(PathBuf::from("/tmp/vidcapture"), "vidcapture".to_string())
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
}
