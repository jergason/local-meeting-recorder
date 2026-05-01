use crate::config::AppConfig;
use crate::transcribe::TranscriptionResult;
use serde::{Deserialize, Serialize};

/// Summary output from the LLM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SummaryResult {
    pub summary: String,
    pub key_points: Vec<String>,
    pub action_items: Vec<String>,
}

const SYSTEM_PROMPT: &str = "You are a helpful assistant that summarizes meeting transcripts. Provide a concise summary, key points, and action items. Do not include any thinking or reasoning - just provide the formatted output directly.";

const OLLAMA_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "qwen3.5:latest";

/// Pick the Ollama model tag from config, falling back to the default.
fn ollama_model_name(config: &AppConfig) -> String {
    config
        .llm_model
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatResponseMessage,
}

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

/// Summarize a transcript by calling Ollama's HTTP API at localhost:11434.
/// The Ollama process must be running (the Tauri app spawns it as a sidecar
/// in production; in dev, run `ollama serve` separately).
pub async fn summarize_transcript(
    transcript: &TranscriptionResult,
) -> Result<SummaryResult, String> {
    let config = AppConfig::load();
    let model = ollama_model_name(&config);
    let user_prompt = build_user_prompt(transcript);

    println!("Summarizing with Ollama model: {}", model);
    println!("Prompt length: {} chars", user_prompt.len());

    let req = ChatRequest {
        model: &model,
        messages: vec![
            ChatMessage { role: "system", content: SYSTEM_PROMPT },
            ChatMessage { role: "user", content: &user_prompt },
        ],
        stream: false,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("http client: {}", e))?;

    let resp = client
        .post(format!("{}/api/chat", OLLAMA_BASE_URL))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}. Is `ollama serve` running?", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {}: {}", status, body));
    }

    let chat: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

    let output = chat.message.content;
    println!("Generated {} chars of output", output.len());

    Ok(parse_summary(&output))
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
    fn test_build_user_prompt_formats_speakers() {
        let transcript = crate::transcribe::TranscriptionResult {
            segments: vec![
                crate::transcribe::TranscriptSegment {
                    id: "seg_0".into(),
                    text: "Hello team".into(),
                    start_time: 0.0,
                    end_time: 1.0,
                    speaker: "Me".into(),
                },
                crate::transcribe::TranscriptSegment {
                    id: "seg_1".into(),
                    text: "Hi there".into(),
                    start_time: 1.0,
                    end_time: 2.0,
                    speaker: "Meeting".into(),
                },
            ],
            full_text: "Hello team Hi there".into(),
            duration: 2.0,
        };

        let prompt = build_user_prompt(&transcript);
        assert!(prompt.contains("[Me] Hello team"));
        assert!(prompt.contains("[Meeting] Hi there"));
        assert!(prompt.contains("## Summary"));
    }

    #[test]
    fn test_ollama_model_name_uses_default_when_unset() {
        let config = AppConfig {
            setup_complete: false,
            whisper_model: None,
            llm_model: None,
        };
        assert_eq!(ollama_model_name(&config), DEFAULT_MODEL);
    }

    #[test]
    fn test_ollama_model_name_uses_config() {
        let config = AppConfig {
            setup_complete: true,
            whisper_model: None,
            llm_model: Some("qwen3:8b".into()),
        };
        assert_eq!(ollama_model_name(&config), "qwen3:8b");
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

    /// E2E: run the summarizer against a real running Ollama instance.
    /// Skips gracefully if Ollama isn't reachable at localhost:11434.
    ///
    /// Run with:  cargo test --lib summarize -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_summarize_transcript_e2e() {
        let transcript = crate::transcribe::TranscriptionResult {
            segments: vec![
                crate::transcribe::TranscriptSegment {
                    id: "seg_0".into(),
                    text: "Let's sync on the launch. The blocker is the auth migration.".into(),
                    start_time: 0.0,
                    end_time: 4.0,
                    speaker: "Me".into(),
                },
                crate::transcribe::TranscriptSegment {
                    id: "seg_1".into(),
                    text: "I'll finish the migration script by Friday and send a PR.".into(),
                    start_time: 4.0,
                    end_time: 8.0,
                    speaker: "Meeting".into(),
                },
                crate::transcribe::TranscriptSegment {
                    id: "seg_2".into(),
                    text: "Great. Then we can ship Monday. I'll book the launch review.".into(),
                    start_time: 8.0,
                    end_time: 12.0,
                    speaker: "Me".into(),
                },
            ],
            full_text: String::new(),
            duration: 12.0,
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = match rt.block_on(summarize_transcript(&transcript)) {
            Ok(r) => r,
            Err(e) if e.contains("Is `ollama serve` running") => {
                println!("SKIP: Ollama not reachable at localhost:11434 ({})", e);
                return;
            }
            Err(e) => panic!("summarize failed: {}", e),
        };

        println!("--- Summary ---\n{}", result.summary);
        println!("--- Key Points ({}) ---", result.key_points.len());
        for p in &result.key_points {
            println!("  - {}", p);
        }
        println!("--- Action Items ({}) ---", result.action_items.len());
        for a in &result.action_items {
            println!("  - {}", a);
        }

        assert!(
            !result.summary.is_empty()
                || !result.key_points.is_empty()
                || !result.action_items.is_empty(),
            "model produced entirely empty output"
        );
    }
}
