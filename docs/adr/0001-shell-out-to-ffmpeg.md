# Shell out to ffmpeg via Command

We chose to shell out to ffmpeg via `std::process::Command` instead of using Rust ffmpeg bindings (`ffmpeg-next`/`ffmpeg-sys`) or native macOS APIs (ScreenCaptureKit/AVFoundation).

**Why**: ffmpeg handles screen capture (avfoundation), audio capture, H.264 encoding, and segment splitting battle-tested. Shell-out avoids C linking complexity on macOS and gives us a working prototype fast. The tradeoff is requiring `brew install ffmpeg` as a system dependency.

**Alternatives considered**:
- `ffmpeg-next` (Rust bindings) — rejected due to linking headaches and macOS-specific build issues.
- Native ScreenCaptureKit + AVFoundation — rejected due to sparse Rust crate support and significantly more code.
