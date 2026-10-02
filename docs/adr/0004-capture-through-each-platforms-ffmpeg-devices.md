# Capture through each platform's ffmpeg input devices, with system audio optional

`vidcapture start` records on macOS, Linux, and Windows by choosing the ffmpeg input devices for the platform it runs on, and still only shells out to ffmpeg (ADR 0001):

| Platform | Screen | Audio | Graceful stop |
| --- | --- | --- | --- |
| macOS | `avfoundation` | `avfoundation` | `SIGINT` |
| Linux | `x11grab` on `$DISPLAY` | `pulse` (PulseAudio, or PipeWire's pulse server) | `SIGINT` |
| Windows | `gdigrab` on the desktop | `dshow` | `q` on ffmpeg's stdin |

System audio and the microphone are each optional. A capture session records whichever is there, mixed into one AAC track as before. Before recording starts, it warns about each one that is missing and says how to add it. The only failures are no ffmpeg, and on Linux no X11 display.

**Why system audio is optional rather than required**: before this change, macOS refused to record without BlackHole 2ch, so `start` needed a third-party audio driver and a Multi-Output Device setup. That was the only part of the tool tied to one machine's configuration. ffmpeg on its own cannot record what macOS is playing; that takes a loopback device. Windows is the same unless the sound driver's "Stereo Mix" is enabled. Linux is the exception: every PulseAudio or PipeWire output has a monitor source, so system audio needs nothing extra there. Making system audio optional lets `start` work on a stock install everywhere, and anyone who wants system audio adds a loopback device once. With BlackHole and a microphone present, the macOS command is exactly the one built before the change, and a unit test pins it.

**How each platform finds its devices**: the same way macOS always did, by asking ffmpeg and parsing the answer. On Linux, `ffmpeg -sources pulse` lists the PulseAudio sources. System audio is `@DEFAULT_MONITOR@`, which the server resolves to the monitor of whichever output is the default, so the recording follows what the user hears. The microphone is the default source unless that default is itself a monitor. On Windows, `ffmpeg -list_devices true -f dshow -i dummy` lists the DirectShow devices. System audio is a loopback device recognized by name ("Stereo Mix" in several display languages, `virtual-audio-capturer`, VB-CABLE). The microphone is chosen as on macOS, never a loopback.

**Why Windows stops with `q`**: ffmpeg writes the MP4 trailer only on a graceful exit. Unix gets that from `SIGINT`. Windows has no signal to send a console child, and `Child::kill` terminates it mid-write and leaves an unplayable file. ffmpeg also reads its stdin as a keyboard, and `q` makes it finish the file and exit 0. So the capture process always runs with stdin piped. On Windows a stop writes `q` to that pipe; on Unix the pipe also stops ffmpeg from reading the key presses the stop-key listener is waiting for.

**Why Linux and Windows add `-pix_fmt yuv420p` and `-thread_queue_size`**: `x11grab` and `gdigrab` produce RGB frames. From those, libx264 keeps 4:4:4 chroma and writes a High 4:4:4 stream that most players, browsers and Windows' built-in one included, will not open. The live audio inputs also need more queue than ffmpeg's default, or they drop samples while the encoder waits on the slower screen input. The macOS command keeps neither option, so a setup that works today records the same file as before.

**Known limits**: on Linux, `x11grab` records X11. Under a Wayland session it sees only XWayland windows, so `start` warns when `XDG_SESSION_TYPE` is `wayland`. Wayland capture would need the xdg-desktop-portal and PipeWire screencast, which ffmpeg has no input device for.

**Alternatives considered**:

- **Native system-audio capture (ScreenCaptureKit or Core Audio process taps on macOS, WASAPI loopback on Windows) piped into ffmpeg**: rejected for now. It is the only way to get system audio with no extra setup on macOS and Windows. It also brings back what ADR 0001 rejected: Apple-framework linking, a Swift or Objective-C bridge, and a second clock to keep in sync with ffmpeg's screen input. That is a lot of platform-specific code that can only be tested on each machine. Making the loopback optional removes the dependency without it, and does not rule it out later.
- **Keep BlackHole required on macOS, add Linux and Windows beside it**: rejected. It keeps macOS as the one platform where `start` fails on a fresh install, which was the problem.
- **`ddagrab` instead of `gdigrab` on Windows**: faster, but it is a filter that needs Direct3D 11 and a hardware download step. `gdigrab` is in every Windows build of ffmpeg and needs no GPU.
- **ALSA instead of PulseAudio on Linux**: ALSA has no monitor of the output, so system audio would need a loopback module the user has to load. PulseAudio and PipeWire's pulse server are what desktop Linux ships.
