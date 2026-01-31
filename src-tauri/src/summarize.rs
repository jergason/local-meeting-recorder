use crate::config::AppConfig;
use crate::transcribe::TranscriptionResult;
use mistralrs::{GgufModelBuilder, TextMessageRole, TextMessages};

/// Summary output from the LLM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SummaryResult {
    pub summary: String,
    pub key_points: Vec<String>,
    pub action_items: Vec<String>,
}

const SYSTEM_PROMPT: &str = "You are a helpful assistant that summarizes meeting transcripts. Provide a concise summary, key points, and action items. Do not include any thinking or reasoning - just provide the formatted output directly.";

/// Build the user prompt with transcript
fn build_user_prompt(transcript: &TranscriptionResult) -> String {
    let mut formatted_transcript = String::new();
    for seg in &transcript.segments {
        formatted_transcript.push_str(&format!("[{}] {}\n", seg.speaker, seg.text));
    }

    format!(
        r#"Please summarize the following meeting transcript:

{}

Provide your response in this exact format:
## Summary
[2-3 sentence overview of the meeting]

## Key Points
- [point 1]
- [point 2]
- [point 3]

## Action Items
- [ ] [action 1]
- [ ] [action 2]"#,
        formatted_transcript
    )
}

/// Parse the LLM output into structured summary
fn parse_summary(output: &str) -> SummaryResult {
    let mut summary = String::new();
    let mut key_points = Vec::new();
    let mut action_items = Vec::new();

    let mut current_section = "";

    // Strip any <think>...</think> blocks from Qwen3 reasoning mode
    let cleaned = strip_thinking_blocks(output);

    for line in cleaned.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## Summary") {
            current_section = "summary";
        } else if trimmed.starts_with("## Key Points") {
            current_section = "key_points";
        } else if trimmed.starts_with("## Action Items") {
            current_section = "action_items";
        } else if !trimmed.is_empty() {
            match current_section {
                "summary" => {
                    if !summary.is_empty() {
                        summary.push(' ');
                    }
                    summary.push_str(trimmed);
                }
                "key_points" => {
                    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                        key_points.push(trimmed[2..].to_string());
                    } else if !trimmed.starts_with('#') {
                        key_points.push(trimmed.to_string());
                    }
                }
                "action_items" => {
                    let item = trimmed
                        .trim_start_matches("- [ ] ")
                        .trim_start_matches("- [x] ")
                        .trim_start_matches("- ")
                        .trim_start_matches("* ");
                    if !item.is_empty() && !item.starts_with('#') {
                        action_items.push(item.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    SummaryResult {
        summary,
        key_points,
        action_items,
    }
}

/// Remove <think>...</think> blocks that Qwen3 may emit
fn strip_thinking_blocks(text: &str) -> String {
    let mut result = String::new();
    let mut in_think = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Check for <think>
            let mut tag = String::from("<");
            while let Some(&next) = chars.peek() {
                if next == '>' {
                    tag.push(chars.next().unwrap());
                    break;
                }
                tag.push(chars.next().unwrap());
            }

            if tag == "<think>" {
                in_think = true;
            } else if tag == "</think>" {
                in_think = false;
            } else if !in_think {
                result.push_str(&tag);
            }
        } else if !in_think {
            result.push(c);
        }
    }

    result
}

/// Summarize a transcript using the local LLM
pub async fn summarize_transcript(transcript: &TranscriptionResult) -> Result<SummaryResult, String> {
    let config = AppConfig::load();

    let model_path = config
        .llm_model_path()
        .ok_or("LLM model not found. Please run setup first.")?;

    println!("Loading LLM model from: {:?}", model_path);

    let model_dir = model_path
        .parent()
        .ok_or("Invalid model path")?;
    let model_file = model_path
        .file_name()
        .ok_or("Invalid model filename")?
        .to_str()
        .ok_or("Invalid model filename encoding")?;

    // mistral.rs auto-detects chat template from Qwen3 GGUF
    let model = GgufModelBuilder::new(model_dir, vec![model_file])
        .build()
        .await
        .map_err(|e| format!("Failed to load model: {}", e))?;

    let user_prompt = build_user_prompt(transcript);
    println!("Prompt length: {} chars", user_prompt.len());

    let messages = TextMessages::new()
        .add_message(TextMessageRole::System, SYSTEM_PROMPT)
        .add_message(TextMessageRole::User, &user_prompt);

    let response = model
        .send_chat_request(messages)
        .await
        .map_err(|e| format!("Inference failed: {}", e))?;

    let output = response.choices.first()
        .ok_or("No response choices")?
        .message
        .content
        .as_ref()
        .ok_or("No response content")?;

    println!("Generated {} chars of output", output.len());

    let result = parse_summary(output);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_summary_basic() {
        let output = r#"## Summary
This was a productive meeting about the project.

## Key Points
- Discussed timeline
- Reviewed budget
- Assigned tasks

## Action Items
- [ ] Send follow-up email
- [ ] Schedule next meeting
"#;

        let result = parse_summary(output);

        assert_eq!(result.summary, "This was a productive meeting about the project.");
        assert_eq!(result.key_points.len(), 3);
        assert_eq!(result.key_points[0], "Discussed timeline");
        assert_eq!(result.action_items.len(), 2);
        assert_eq!(result.action_items[0], "Send follow-up email");
    }

    #[test]
    fn test_parse_summary_empty() {
        let output = "";
        let result = parse_summary(output);

        assert!(result.summary.is_empty());
        assert!(result.key_points.is_empty());
        assert!(result.action_items.is_empty());
    }

    #[test]
    fn test_strip_thinking_blocks() {
        let input = "<think>Let me think about this...</think>## Summary\nHere is the summary.";
        let result = strip_thinking_blocks(input);
        assert_eq!(result, "## Summary\nHere is the summary.");
    }

    #[test]
    fn test_parse_summary_with_thinking() {
        let output = r#"<think>
I should analyze this transcript carefully.
</think>
## Summary
This was a productive meeting.

## Key Points
- Point one

## Action Items
- [ ] Do something
"#;

        let result = parse_summary(output);
        assert_eq!(result.summary, "This was a productive meeting.");
        assert_eq!(result.key_points.len(), 1);
        assert_eq!(result.action_items.len(), 1);
    }
}
