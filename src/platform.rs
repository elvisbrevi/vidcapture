//! The operating system vidcapture runs on, and the advice that depends on it.
//!
//! Only `start` records differently per platform; the ffmpeg module builds its
//! command for a given `Platform`. Everything else here is what the user is
//! told: how to install ffmpeg, and which font file a label can be drawn with.

use std::path::PathBuf;

/// An operating system vidcapture can record the screen on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    /// The platform this binary was built for. Unix systems other than macOS
    /// count as Linux: they share its X11 and PulseAudio stack.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    /// The command that installs ffmpeg.
    pub fn ffmpeg_install_command(self) -> &'static str {
        match self {
            Platform::MacOs => "brew install ffmpeg",
            Platform::Linux => "sudo apt install ffmpeg",
            Platform::Windows => "winget install Gyan.FFmpeg",
        }
    }

    /// The error shown when ffmpeg is not on PATH.
    pub fn ffmpeg_not_found_message(self) -> String {
        format!(
            "ffmpeg not found. Install it with: {}",
            self.ffmpeg_install_command()
        )
    }

    /// A font file that ships with the operating system, to show as a
    /// `--font` example.
    pub fn example_font(self) -> &'static str {
        match self {
            Platform::MacOs => "/System/Library/Fonts/Helvetica.ttc",
            Platform::Linux => "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            Platform::Windows => r"C:\Windows\Fonts\arial.ttf",
        }
    }

    /// The font `label` draws with when `--font` is not given.
    ///
    /// `None` leaves the choice to ffmpeg, which finds a system font through
    /// fontconfig on macOS and Linux. Windows builds of ffmpeg do not reliably
    /// have a fontconfig setup that knows the Windows fonts, so name Arial
    /// there directly when it is installed.
    pub fn default_label_font(self) -> Option<PathBuf> {
        match self {
            Platform::Windows => {
                let windows_dir =
                    std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
                let arial = PathBuf::from(windows_dir).join("Fonts").join("arial.ttf");
                arial.is_file().then_some(arial)
            }
            Platform::MacOs | Platform::Linux => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_matches_the_build_target() {
        let expected = if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        };
        assert_eq!(Platform::current(), expected);
    }

    #[test]
    fn ffmpeg_not_found_message_names_the_platform_install_command() {
        assert_eq!(
            Platform::MacOs.ffmpeg_not_found_message(),
            "ffmpeg not found. Install it with: brew install ffmpeg"
        );
        assert!(Platform::Linux
            .ffmpeg_not_found_message()
            .contains("apt install ffmpeg"));
        assert!(Platform::Windows
            .ffmpeg_not_found_message()
            .contains("winget install"));
    }

    #[test]
    fn only_windows_names_a_default_label_font() {
        assert_eq!(Platform::MacOs.default_label_font(), None);
        assert_eq!(Platform::Linux.default_label_font(), None);
    }
}
