use anyhow::Result;
use claude_code_notification::system_sound_names;
use inquire::{validator::Validation, Select, Text};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn system_sounds_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "/System/Library/Sounds"
    } else {
        "/usr/share/sounds/freedesktop/stereo"
    }
}

fn system_sound_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        ".aiff"
    } else {
        ".oga"
    }
}

fn get_claude_settings_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn get_available_system_sounds() -> Vec<String> {
    let sounds_dir = Path::new(system_sounds_dir());
    let ext = system_sound_extension();

    if sounds_dir.exists() {
        let mut sounds = Vec::new();
        if let Ok(entries) = fs::read_dir(sounds_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(ext) {
                        let sound_name = name.trim_end_matches(ext);
                        sounds.push(sound_name.to_string());
                    }
                }
            }
        }
        sounds.sort();
        if !sounds.is_empty() {
            return sounds;
        }
    }

    // Fallback to built-in list
    system_sound_names().iter().map(|s| s.to_string()).collect()
}

fn validate_sound_path(
    sound: &str,
) -> Result<Validation, Box<dyn std::error::Error + Send + Sync + 'static>> {
    if sound.contains('/') {
        let path = Path::new(sound);
        if path.exists() {
            Ok(Validation::Valid)
        } else {
            Ok(Validation::Invalid("Sound file does not exist".into()))
        }
    } else {
        let system_sound_path = Path::new(system_sounds_dir())
            .join(format!("{}{}", sound, system_sound_extension()));
        if system_sound_path.exists() {
            Ok(Validation::Valid)
        } else {
            Ok(Validation::Invalid("System sound does not exist".into()))
        }
    }
}

fn install_icon() -> Result<Option<PathBuf>> {
    // Find the icon in the repo assets
    let exe_path = std::env::current_exe()?;
    let candidates = [
        exe_path
            .parent()
            .and_then(|d| d.parent())
            .map(|d| d.join("assets/claude-icon.png")),
        exe_path
            .parent()
            .map(|d| d.join("assets/claude-icon.png")),
        Some(PathBuf::from("assets/claude-icon.png")),
    ];

    let source = candidates.iter().filter_map(|c| c.as_ref()).find(|p| p.exists());

    if let Some(source_path) = source {
        let home = std::env::var("HOME")?;
        let dest_dir = PathBuf::from(&home).join(".local/share/claude-code-notification");
        fs::create_dir_all(&dest_dir)?;
        let dest = dest_dir.join("claude-icon.png");
        fs::copy(source_path, &dest)?;
        println!("  Icon installed to: {}", dest.display());
        return Ok(Some(dest));
    }

    Ok(None)
}

pub fn run_setup() -> Result<()> {
    println!("Setting up Claude Code notifications\n");

    let available_sounds = get_available_system_sounds();
    let mut sound_options: Vec<String> = available_sounds;
    sound_options.push("Custom file path...".to_string());

    let sound_choice = Select::new("Select a notification sound:", sound_options)
        .with_help_message(
            "Choose a system sound or select 'Custom file path...' to specify your own",
        )
        .prompt()?;

    let selected_sound = if sound_choice == "Custom file path..." {
        let help = if cfg!(target_os = "macos") {
            "Supported formats: .wav, .aiff, .mp3, .m4a"
        } else {
            "Supported formats: .wav, .ogg, .oga, .mp3, .flac"
        };
        Text::new("Enter the path to your custom sound file:")
            .with_help_message(help)
            .with_validator(validate_sound_path)
            .prompt()?
    } else {
        sound_choice
    };

    // Install icon
    println!("\nInstalling notification icon...");
    match install_icon() {
        Ok(Some(_)) => println!("  Icon installed successfully."),
        Ok(None) => println!("  Icon not found in assets/ - notifications will use default icon."),
        Err(e) => println!("  Warning: Could not install icon: {}", e),
    }

    let settings_path = get_claude_settings_path()?;

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let notification_command = if selected_sound.contains('/') {
        format!("claude-code-notification --sound \"{}\"", selected_sound)
    } else {
        format!("claude-code-notification --sound {}", selected_sound)
    };

    settings["hooks"] = json!({
        "Notification": [
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": notification_command
                    }
                ]
            }
        ]
    });

    let settings_json = serde_json::to_string_pretty(&settings)?;
    fs::write(&settings_path, settings_json)?;

    println!("\nClaude Code settings updated successfully!");
    println!("  Settings file: {}", settings_path.display());
    println!("  Selected sound: {}", selected_sound);
    println!("\nYour Claude Code notifications are now configured.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    #[test]
    fn test_generated_settings_match_schema() {
        let schema_response =
            reqwest::blocking::get("https://www.schemastore.org/claude-code-settings.json")
                .expect("Failed to fetch schema from schemastore.org");

        let schema_json: Value = schema_response.json().expect("Failed to parse schema JSON");

        let validator =
            jsonschema::validator_for(&schema_json).expect("Failed to compile JSON schema");

        let notification_command = "claude-code-notification --sound message-new-instant";

        let test_settings = json!({
            "hooks": {
                "Notification": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": notification_command
                            }
                        ]
                    }
                ]
            }
        });

        if validator.is_valid(&test_settings) {
            println!(
                "Generated settings JSON successfully validates against Claude Code schema"
            );
        } else {
            eprintln!("Schema validation failed:");
            for error in validator.iter_errors(&test_settings) {
                eprintln!("  - {}", error);
                eprintln!("    Instance path: {}", error.instance_path);
                eprintln!("    Schema path: {}", error.schema_path);
            }
            panic!("Generated settings JSON does not match Claude Code schema");
        }
    }

    #[test]
    fn test_settings_creation_and_validation() {
        let temp_dir = TempDir::new().expect("Failed to create temporary directory");
        let temp_settings_path = temp_dir.path().join("test_settings.json");

        let notification_command = "claude-code-notification --sound complete";
        let mut settings = json!({});

        settings["hooks"] = json!({
            "Notification": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": notification_command
                        }
                    ]
                }
            ]
        });

        let settings_json =
            serde_json::to_string_pretty(&settings).expect("Failed to serialize settings");
        std::fs::write(&temp_settings_path, &settings_json)
            .expect("Failed to write test settings file");

        let read_settings: Value = serde_json::from_str(
            &std::fs::read_to_string(&temp_settings_path)
                .expect("Failed to read test settings file"),
        )
        .expect("Failed to parse read settings");

        assert!(read_settings["hooks"].is_object());
        assert!(read_settings["hooks"]["Notification"].is_array());

        let notification_hooks = &read_settings["hooks"]["Notification"];
        assert_eq!(notification_hooks.as_array().unwrap().len(), 1);

        let first_hook = &notification_hooks[0];
        assert!(first_hook["hooks"].is_array());

        let commands = first_hook["hooks"].as_array().unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0]["type"], "command");
        assert_eq!(commands[0]["command"], notification_command);
    }

    #[test]
    fn test_validate_sound_path_system_sound() {
        let test_sound = if cfg!(target_os = "macos") {
            "Glass"
        } else {
            "bell"
        };
        let result = validate_sound_path(test_sound);
        match result {
            Ok(_validation) => {
                // Valid or invalid depending on whether sound files exist on this system
            }
            Err(_) => {
                panic!("Sound path validation function failed");
            }
        }
    }

    #[test]
    fn test_validate_sound_path_custom_file() {
        let result = validate_sound_path("/nonexistent/file.wav");
        match result {
            Ok(validation) => {
                match validation {
                    inquire::validator::Validation::Invalid(_) => {}
                    inquire::validator::Validation::Valid => {
                        panic!("Validation should fail for non-existent file");
                    }
                }
            }
            Err(_) => {
                panic!("Sound path validation function failed");
            }
        }
    }

    #[test]
    fn test_get_available_system_sounds() {
        let sounds = get_available_system_sounds();
        assert!(!sounds.is_empty());
    }
}
