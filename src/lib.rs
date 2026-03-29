pub mod error;

use anyhow::Result;
use notify_rust::{Hint, Notification, Timeout, Urgency};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::Command;
use std::thread;

pub use error::{NotificationError, NotificationResult};

#[derive(Debug, Deserialize, Serialize)]
pub struct NotificationInput {
    pub session_id: String,
    pub transcript_path: String,
    pub message: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Sound {
    System(String),
    Custom(String),
}

impl Default for Sound {
    fn default() -> Self {
        Sound::System(default_sound_name().to_string())
    }
}

/// Returns the default sound name for the current platform.
pub fn default_sound_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Glass"
    } else {
        "message-new-instant"
    }
}

/// Returns the list of available system sound names for the current platform.
pub fn system_sound_names() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &[
            "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop",
            "Purr", "Sosumi", "Submarine", "Tink",
        ]
    } else {
        &[
            "bell",
            "complete",
            "message-new-instant",
            "message",
            "dialog-information",
            "dialog-warning",
            "dialog-error",
            "window-attention",
            "device-added",
            "service-login",
            "alarm-clock-elapsed",
            "camera-shutter",
            "screen-capture",
            "phone-incoming-call",
            "trash-empty",
        ]
    }
}

impl Sound {
    pub fn from_name(name: &str) -> Self {
        if name.contains('/') {
            Sound::Custom(name.to_string())
        } else {
            Sound::System(name.to_string())
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Sound::System(name) => name,
            Sound::Custom(path) => path,
        }
    }

    /// Resolves the sound to a file path for the current platform.
    pub fn resolve_path(&self) -> String {
        match self {
            Sound::Custom(path) => path.clone(),
            Sound::System(name) => {
                if cfg!(target_os = "macos") {
                    format!("/System/Library/Sounds/{}.aiff", name)
                } else {
                    format!("/usr/share/sounds/freedesktop/stereo/{}.oga", name)
                }
            }
        }
    }
}

pub fn main<R: Read>(mut stdin: R, sound: Sound) -> Result<()> {
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;

    let input: NotificationInput = serde_json::from_str(&buffer)?;

    send_notification(&input, &sound)?;

    Ok(())
}


fn send_notification(input: &NotificationInput, sound: &Sound) -> Result<()> {
    let title = input.title.as_deref().unwrap_or("Claude Code");

    let sound_clone = sound.clone();

    // Spawn a thread to play the sound in parallel
    let sound_handle = thread::spawn(move || {
        if let Err(e) = play_sound(&sound_clone) {
            eprintln!("Warning: Failed to play sound: {}", e);
        }
    });

    // Show the notification (this happens in parallel with sound)
    let mut notification = Notification::new();
    notification
        .summary(title)
        .body(&input.message)
        .appname("Claude Code")
        .hint(Hint::DesktopEntry("claude-code-notification".to_string()))
        .hint(Hint::Category("im.received".to_string()))
        .urgency(Urgency::Normal)
        .timeout(Timeout::Milliseconds(5000))
        .icon("claude-code-notification");

    let notification_result = notification.show();

    // Wait for the sound thread to complete
    if let Err(e) = sound_handle.join() {
        eprintln!("Warning: Sound thread panicked: {:?}", e);
    }

    notification_result?;
    Ok(())
}

fn find_audio_player() -> Option<(&'static str, Vec<&'static str>)> {
    if cfg!(target_os = "macos") {
        return Some(("afplay", vec![]));
    }

    // Linux: try players in order of preference
    let players: &[(&str, &[&str])] = &[
        ("pw-play", &[]),
        ("paplay", &[]),
        ("aplay", &[]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
    ];

    for (player, args) in players {
        if Command::new("which")
            .arg(player)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some((player, args.to_vec()));
        }
    }
    None
}

fn play_sound(sound: &Sound) -> Result<()> {
    let sound_path = sound.resolve_path();

    let Some((player, extra_args)) = find_audio_player() else {
        eprintln!("Warning: No audio player found. Install paplay, pw-play, or aplay for sound support.");
        return Ok(());
    };

    let mut cmd = Command::new(player);
    cmd.arg(&sound_path);
    for arg in &extra_args {
        cmd.arg(arg);
    }

    match cmd.output() {
        Ok(result) => {
            if !result.status.success() {
                eprintln!(
                    "Warning: Failed to play sound '{}'. {} exit code: {:?}",
                    sound_path,
                    player,
                    result.status.code()
                );
            }
        }
        Err(e) => {
            eprintln!(
                "Warning: Failed to execute {} for sound '{}': {}",
                player, sound_path, e
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_valid_input() {
        let input_data = r#"{
            "session_id": "test-session-123",
            "transcript_path": "/path/to/transcript.md",
            "message": "Test notification message",
            "title": "Test Title"
        }"#;

        let input: Result<NotificationInput, _> = serde_json::from_str(input_data);
        assert!(input.is_ok());

        let input = input.unwrap();
        assert_eq!(input.session_id, "test-session-123");
        assert_eq!(input.message, "Test notification message");
        assert_eq!(input.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_parse_missing_title() {
        let input_data = r#"{
            "session_id": "test-session-456",
            "transcript_path": "/path/to/transcript.md",
            "message": "Message without title"
        }"#;

        let input: Result<NotificationInput, _> = serde_json::from_str(input_data);
        assert!(input.is_ok());

        let input = input.unwrap();
        assert_eq!(input.session_id, "test-session-456");
        assert_eq!(input.message, "Message without title");
        assert_eq!(input.title, None);
    }

    #[test]
    fn test_parse_invalid_json() {
        let invalid_json = "{ invalid json }";
        let cursor = Cursor::new(invalid_json);
        let result = main(cursor, Sound::default());

        assert!(result.is_err());
    }

    #[test]
    fn test_empty_input() {
        let empty_input = "";
        let cursor = Cursor::new(empty_input);
        let result = main(cursor, Sound::default());

        assert!(result.is_err());
    }

    #[test]
    fn test_sound_from_name() {
        assert!(matches!(Sound::from_name("bell"), Sound::System(_)));
        assert!(matches!(Sound::from_name("complete"), Sound::System(_)));
        assert!(matches!(
            Sound::from_name("/path/to/sound.wav"),
            Sound::Custom(_)
        ));
    }

    #[test]
    fn test_sound_as_str() {
        assert_eq!(Sound::System("bell".into()).as_str(), "bell");
        assert_eq!(
            Sound::Custom("/path/sound.wav".into()).as_str(),
            "/path/sound.wav"
        );
    }

    #[test]
    fn test_sound_default() {
        let sound = Sound::default();
        assert!(matches!(sound, Sound::System(_)));
    }

    #[test]
    fn test_notification_input_with_special_characters() {
        let input_data = r#"{
            "session_id": "test-session-789",
            "transcript_path": "/path/to/transcript.md",
            "message": "Message with \"quotes\" and special chars",
            "title": "Title with \"quotes\""
        }"#;

        let input: Result<NotificationInput, _> = serde_json::from_str(input_data);
        assert!(input.is_ok());

        let input = input.unwrap();
        assert_eq!(input.message, "Message with \"quotes\" and special chars");
        assert_eq!(input.title, Some("Title with \"quotes\"".to_string()));
    }

    #[test]
    fn test_sound_path_resolution_system() {
        let sound = Sound::System("bell".into());
        let path = sound.resolve_path();
        if cfg!(target_os = "macos") {
            assert_eq!(path, "/System/Library/Sounds/bell.aiff");
        } else {
            assert_eq!(path, "/usr/share/sounds/freedesktop/stereo/bell.oga");
        }
    }

    #[test]
    fn test_sound_path_resolution_custom() {
        let sound = Sound::Custom("/custom/path/sound.wav".into());
        assert_eq!(sound.resolve_path(), "/custom/path/sound.wav");

        let relative = Sound::Custom("./sounds/custom.ogg".into());
        assert_eq!(relative.resolve_path(), "./sounds/custom.ogg");
    }

    #[test]
    fn test_system_sound_names_not_empty() {
        let names = system_sound_names();
        assert!(!names.is_empty());
    }

    #[test]
    fn test_default_sound_name_in_list() {
        let default = default_sound_name();
        let names = system_sound_names();
        assert!(names.contains(&default));
    }
}
