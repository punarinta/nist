//! AI Agent for command generation using LLM
//!
//! This module integrates with LLM vendors to generate native shell commands
//! based on user prompts and context.

use crate::settings::{ExternalVendor, Settings};
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::{ChatMessage, StructuredOutputFormat};
use llm::error::LLMError;
use serde_json::json;
use std::env;

/// Default URL for the AI service
pub const DEFAULT_AI_URL: &str = "http://localhost:1314/v1";

/// Error types for AI agent operations
#[derive(Debug)]
pub enum AgentError {
    /// No vendor with the specified name found in settings
    VendorNotFound(String),
    /// LLM API error
    LlmError(LLMError),
    /// Failed to get current directory
    CurrentDirError(std::io::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::VendorNotFound(name) => write!(f, "Vendor '{}' not found in settings", name),
            AgentError::LlmError(e) => write!(f, "LLM error: {}", e),
            AgentError::CurrentDirError(e) => write!(f, "Current directory error: {}", e),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<LLMError> for AgentError {
    fn from(err: LLMError) -> Self {
        AgentError::LlmError(err)
    }
}

/// Get the vendor configuration from settings, or return a default "nisdos" vendor if not found
fn get_vendor(settings: &Settings, vendor_name: &str) -> Result<ExternalVendor, AgentError> {
    // Try to find the vendor in settings
    if let Some(vendor) = settings.external.iter().find(|v| v.name == vendor_name) {
        return Ok(vendor.clone());
    }

    // If vendor is "nisdos" and not found, return a default vendor
    if vendor_name == "nisdos" {
        return Ok(ExternalVendor {
            name: "nisdos".to_string(),
            api_key: String::new(),
            url: DEFAULT_AI_URL.to_string(),
        });
    }

    // For other vendors, return an error
    Err(AgentError::VendorNotFound(vendor_name.to_string()))
}

/// Get OS information string
fn get_os_info() -> Result<String, AgentError> {
    let os_name = if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Unknown"
    };

    // Try to get OS version
    let os_version = if let Ok(info) = sys_info::os_release() {
        format!("{} {}", os_name, info)
    } else {
        os_name.to_string()
    };

    Ok(os_version)
}

/// Get current working directory as a string
fn get_current_dir() -> Result<String, AgentError> {
    Ok(env::current_dir().map_err(AgentError::CurrentDirError)?.to_string_lossy().to_string())
}

/// Trim triple quotes from a command string if present
///
/// Removes surrounding ``` or """ from commands, also handling optional language specifiers like ```bash
fn trim_triple_quotes(command: &str) -> String {
    let trimmed = command.trim();

    // Check for ``` (with optional language specifier)
    if trimmed.starts_with("```") {
        let without_prefix = trimmed.strip_prefix("```").unwrap();
        // Remove optional language identifier (e.g., bash, sh, etc.)
        let without_lang = without_prefix.trim_start_matches(|c: char| c.is_alphanumeric());
        let without_lang = without_lang.trim_start();

        // Remove trailing ```
        if let Some(stripped) = without_lang.strip_suffix("```") {
            return stripped.trim().to_string();
        }
        return without_lang.trim().to_string();
    }

    // Check for """
    if trimmed.starts_with("\"\"\"") && trimmed.ends_with("\"\"\"") && trimmed.len() > 6 {
        return trimmed[3..trimmed.len() - 3].trim().to_string();
    }

    trimmed.to_string()
}

/// Generate a native command using LLM based on user prompt
///
/// # Arguments
/// * `settings` - Application settings containing vendor configuration
/// * `user_prompt` - The user's natural language command request
/// * `history` - Recent command history (up to 10 items)
///
/// # Returns
/// A native shell command as a string
///
/// # Examples
/// ```no_run
/// use nist::settings::{Settings, ExternalVendor};
/// use nist::ai::agent::generate_command;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut settings = Settings::default();
/// settings.external.push(ExternalVendor {
///     name: "nisdos".to_string(),
///     api_key: "your-api-key".to_string(),
///     url: "http://localhost:1314/v1".to_string(),
/// });
///
/// let history = vec!["ls -la".to_string(), "cd /tmp".to_string()];
/// let command = generate_command(&settings, "list all files", &history).await?;
/// println!("Generated command: {}", command);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
/// Returns an error if the vendor is not found or LLM API fails.
/// Empty API keys are sent as "-" to bypass client-side validation, allowing the remote server to handle authentication.
pub async fn generate_command(settings: &Settings, user_prompt: &str, history: &[String]) -> Result<String, AgentError> {
    // Get vendor configuration
    let vendor = get_vendor(settings, "nisdos")?;

    // Determine the URL to use
    let url = if vendor.url.is_empty() { DEFAULT_AI_URL } else { &vendor.url };

    // Get context information
    let os_info = get_os_info()?;
    let current_dir = get_current_dir()?;

    // Determine OS name for expert instruction
    let os_expert = if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Unix"
    };

    // Take up to 10 most recent history items
    let recent_history: Vec<&String> = history.iter().take(10).collect();

    // Build context message
    let mut context = String::new();
    context.push_str(&format!("Operating System: {}\n", os_info));
    context.push_str(&format!("Current Directory: {}\n", current_dir));

    if !recent_history.is_empty() {
        context.push_str("\nRecent Command History:\n");
        for (i, cmd) in recent_history.iter().enumerate() {
            context.push_str(&format!("{}. {}\n", i + 1, cmd));
        }
    }

    // Build the system prompt with extremely strict instructions
    // Note: Using JSON mode, so the LLM will automatically format the response as JSON
    let system_prompt = format!(
        "You are a {} command generator. Your job is to generate shell commands.\n\
        The response will be formatted as JSON with a 'command' field.\n\
        \n\
        CRITICAL RULES:\n\
        1. The 'command' field must contain ONLY the raw, executable shell command\n\
        2. NO explanations, descriptions, or comments are allowed\n\
        3. NO markdown formatting (NO ``` or ```bash)\n\
        4. NO prefixes like 'Command:', 'Here:', 'Try:', etc.\n\
        5. Command must be directly executable on {}\n\
        6. Multiple commands must be chained with && or ; on ONE line\n\
        \n\
        CORRECT EXAMPLES:\n\
        ✓ {{\"command\": \"uname -r\"}}\n\
        ✓ {{\"command\": \"ls -la | grep test\"}}\n\
        ✓ {{\"command\": \"cd /tmp && find . -name '*.log'\"}}\n\
        \n\
        The command field should contain clean, executable shell commands without any wrapper text.",
        os_expert, os_expert
    );

    // Build the user message with context
    let user_message = format!("{}\n{}", context, user_prompt);

    // Use "-" as API key if empty to bypass client-side validation
    // This allows the remote server to handle authentication
    let api_key = if vendor.api_key.is_empty() { "-" } else { &vendor.api_key };

    // Define JSON schema for structured output
    let schema = StructuredOutputFormat {
        name: "CommandOutput".to_string(),
        description: Some("A shell command to be executed".to_string()),
        schema: Some(json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The raw shell command to execute"
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })),
        strict: Some(true),
    };

    // Initialize LLM with OpenAI backend (using Nisdos-L model) with JSON output mode
    let provider = LLMBuilder::new()
        .backend(LLMBackend::OpenAI)
        .api_key(api_key)
        .base_url(url)
        .model("Nisdos-L")
        .system(&system_prompt)
        .schema(schema)
        .temperature(0.0)
        .build()?;

    // Create chat messages
    let messages = vec![ChatMessage::user().content(&user_message).build()];

    // Get completion
    let response = provider.chat(&messages).await?;

    // Extract the command from JSON response
    let raw_output = response.text().unwrap_or_default();

    // Parse JSON response and extract the "command" field
    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&raw_output) {
        if let Some(command) = json_value.get("command").and_then(|v| v.as_str()) {
            return Ok(trim_triple_quotes(command));
        }
    }

    // Fallback: if JSON parsing fails, use the raw output (backward compatibility)
    Ok(trim_triple_quotes(&raw_output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_os_info() {
        let os_info = get_os_info();
        assert!(os_info.is_ok());
        let info = os_info.unwrap();
        assert!(!info.is_empty());
    }

    #[test]
    fn test_get_current_dir() {
        let dir = get_current_dir();
        assert!(dir.is_ok());
        let path = dir.unwrap();
        assert!(!path.is_empty());
    }

    #[test]
    fn test_get_vendor_default_nisdos() {
        // When nisdos vendor is not configured, a default should be provided
        let settings = Settings::default();
        let result = get_vendor(&settings, "nisdos");
        assert!(result.is_ok());
        let vendor = result.unwrap();
        assert_eq!(vendor.name, "nisdos");
        assert_eq!(vendor.url, DEFAULT_AI_URL);
        assert_eq!(vendor.api_key, ""); // Default has empty API key
    }

    #[test]
    fn test_get_vendor_not_found_other_vendor() {
        // Non-nisdos vendors should still return an error if not configured
        let settings = Settings::default();
        let result = get_vendor(&settings, "openai");
        assert!(matches!(result, Err(AgentError::VendorNotFound(_))));
    }

    #[test]
    fn test_get_vendor_found() {
        let mut settings = Settings::default();
        settings.external.push(ExternalVendor {
            name: "nisdos".to_string(),
            api_key: "test_key".to_string(),
            url: "http://test.com".to_string(),
        });

        let result = get_vendor(&settings, "nisdos");
        assert!(result.is_ok());
        let vendor = result.unwrap();
        assert_eq!(vendor.name, "nisdos");
        assert_eq!(vendor.api_key, "test_key");
    }

    #[test]
    fn test_json_command_extraction_valid() {
        // Test valid JSON response with command field
        let json_response = r#"{"command": "ls -la"}"#;
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_response);
        assert!(parsed.is_ok());
        let json_value = parsed.unwrap();
        let command = json_value.get("command").and_then(|v| v.as_str());
        assert_eq!(command, Some("ls -la"));
    }

    #[test]
    fn test_json_command_extraction_with_complex_command() {
        // Test JSON with complex shell command
        let json_response = r#"{"command": "find . -name '*.log' | grep -v tmp"}"#;
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_response);
        assert!(parsed.is_ok());
        let json_value = parsed.unwrap();
        let command = json_value.get("command").and_then(|v| v.as_str());
        assert_eq!(command, Some("find . -name '*.log' | grep -v tmp"));
    }

    #[test]
    fn test_json_command_extraction_invalid_json() {
        // Test that invalid JSON returns an error
        let json_response = r#"not valid json"#;
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_response);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_json_command_extraction_missing_command_field() {
        // Test JSON without command field
        let json_response = r#"{"result": "uname -r"}"#;
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_response);
        assert!(parsed.is_ok());
        let json_value = parsed.unwrap();
        let command = json_value.get("command").and_then(|v| v.as_str());
        assert_eq!(command, None);
    }

    #[test]
    fn test_json_command_extraction_empty_command() {
        // Test JSON with empty command field
        let json_response = r#"{"command": ""}"#;
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_response);
        assert!(parsed.is_ok());
        let json_value = parsed.unwrap();
        let command = json_value.get("command").and_then(|v| v.as_str());
        assert_eq!(command, Some(""));
    }

    #[test]
    fn test_trim_triple_quotes_with_backticks() {
        assert_eq!(trim_triple_quotes("```ls -la```"), "ls -la");
        assert_eq!(trim_triple_quotes("```bash\nls -la\n```"), "ls -la");
        assert_eq!(trim_triple_quotes("```sh\nfind . -name '*.log'\n```"), "find . -name '*.log'");
        assert_eq!(trim_triple_quotes("  ```  ls -la  ```  "), "ls -la");
    }

    #[test]
    fn test_trim_triple_quotes_with_double_quotes() {
        assert_eq!(trim_triple_quotes("\"\"\"ls -la\"\"\""), "ls -la");
        assert_eq!(trim_triple_quotes("  \"\"\"  ls -la  \"\"\"  "), "ls -la");
    }

    #[test]
    fn test_trim_triple_quotes_no_quotes() {
        assert_eq!(trim_triple_quotes("ls -la"), "ls -la");
        assert_eq!(trim_triple_quotes("  ls -la  "), "ls -la");
    }

    #[test]
    fn test_trim_triple_quotes_partial_quotes() {
        // Only removes quotes if they're properly paired
        assert_eq!(trim_triple_quotes("```ls -la"), "ls -la");
        assert_eq!(trim_triple_quotes("ls -la```"), "ls -la```");
    }

    #[tokio::test]
    #[ignore] // This test requires a real API key and connection
    async fn test_generate_command_example() {
        // Example of how to use generate_command
        // This test is ignored by default as it requires an actual API setup

        let mut settings = Settings::default();
        settings.external.push(ExternalVendor {
            name: "nisdos".to_string(),
            api_key: "test-key".to_string(), // Replace with actual key for testing
            url: DEFAULT_AI_URL.to_string(),
        });

        let history = vec!["ls -la".to_string(), "cd /home".to_string(), "pwd".to_string()];

        let user_prompt = "list all files in the current directory";

        // This would make an actual API call
        let result = generate_command(&settings, user_prompt, &history).await;

        // In a real test with a mock server, we would verify the result
        // For now, we just ensure the function signature is correct
        match result {
            Ok(command) => println!("Generated command: {}", command),
            Err(e) => println!("Error (expected without real API): {}", e),
        }
    }
}
