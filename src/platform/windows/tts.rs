/// Text-to-speech via PowerShell System.Speech.
///
/// Shells out to avoid COM initialization complexity.

pub fn speak(text: &str, _voice: Option<&str>) -> Result<(), String> {
    let escaped = text.replace('\'', "''").replace('"', "`\"");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{}')",
        escaped
    );

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| format!("Failed to run powershell: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}
