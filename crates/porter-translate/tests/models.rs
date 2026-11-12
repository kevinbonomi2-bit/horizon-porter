// The models `translate` asks, against a stand-in server on this computer: what a
// request carries, what a cut-off answer is asked next, and what the tries cost.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use porter_translate::ai::model::{self, Anthropic, Compatible, Model, Usage};

/// What a stand-in server was sent: each request's path and body.
type Seen = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

/// A server that answers each request with the next of `answers` (a status, extra
/// header lines and a JSON body), and keeps what it was sent.
fn server(answers: Vec<(u16, &'static str, serde_json::Value)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, headers, answer) in answers {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            log.lock().unwrap().push((
                path,
                serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
            ));
            let text = answer.to_string();
            let mut stream = reader.into_inner();
            write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            )
            .unwrap();
        }
    });
    (base, seen)
}

#[test]
fn a_long_prompt_costs_claude_haiku_five_times_as_much_a_token() {
    assert_eq!(
        model::price("claude-haiku-5-5", 100_000),
        Some((0.10, 0.50))
    );
    assert_eq!(
        model::price("claude-haiku-5-5", 100_001),
        Some((0.50, 2.50))
    );
    // The other models have one price at any length.
    assert_eq!(
        model::price("claude-sonnet-5-5", 500_000),
        model::price("claude-sonnet-5-5", 5)
    );
    // The cache: a write costs a quarter more than reading it plainly, a read a tenth;
    // and it is the whole prompt, cached or not, that picks the prices.
    let short = Usage {
        input: 40_000,
        cache_write: 20_000,
        cache_read: 30_000,
        output: 4_000,
    };
    let dollars = short.dollars("claude-haiku-5-5").unwrap();
    assert!(
        (dollars - (0.004 + 0.0025 + 0.0003 + 0.002)).abs() < 1e-9,
        "{dollars}"
    );
    let long = Usage {
        input: 40_000,
        cache_read: 70_000,
        ..short
    };
    assert_eq!(long.prompt(), 130_000);
    let dollars = long.dollars("claude-haiku-5-5").unwrap();
    assert!(
        (dollars - (0.02 + 0.0125 + 0.0035 + 0.01)).abs() < 1e-9,
        "{dollars}"
    );
    assert_eq!(short.dollars("a-model-of-someone-elses"), None);
}

#[test]
fn an_answer_lost_to_thinking_is_asked_again_with_less_of_it_and_both_are_paid() {
    let (base, seen) = server(vec![
        (
            200,
            "",
            // All the room went on thinking, which comes back empty.
            serde_json::json!({
                "stop_reason": "max_tokens",
                "content": [{ "type": "thinking", "thinking": "", "signature": "s" }],
                "usage": { "input_tokens": 30_000, "cache_creation_input_tokens": 20_000, "output_tokens": 16_000 },
            }),
        ),
        (
            200,
            "",
            serde_json::json!({
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "```rust\nfn a() {}\n```" }],
                "usage": { "input_tokens": 30_000, "cache_read_input_tokens": 20_000, "output_tokens": 2_000 },
            }),
        ),
    ]);
    let claude = Anthropic::with_key("test", None, None).at(&base);
    let reply = claude
        .complete("instructions", "the shared part", "the function")
        .unwrap();
    assert!(reply.text.contains("fn a()"));
    assert_eq!(reply.cut_off, None);
    // Both requests' tokens, and what both cost at the short prompt's prices.
    assert_eq!(reply.input_tokens, 100_000);
    assert_eq!(reply.output_tokens, 18_000);
    assert_eq!(reply.cached_tokens, 20_000);
    let first = 30_000.0 * 0.1 / 1e6 + 20_000.0 * 0.125 / 1e6 + 16_000.0 * 0.5 / 1e6;
    let second = 30_000.0 * 0.1 / 1e6 + 20_000.0 * 0.01 / 1e6 + 2_000.0 * 0.5 / 1e6;
    assert!((reply.dollars.unwrap() - (first + second)).abs() < 1e-9);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let (path, one) = &seen[0];
    assert_eq!(path, "/v1/messages");
    // The shared part is the cached end of the system prompt; the function is the message.
    assert_eq!(one["system"][0]["text"], "instructions");
    assert_eq!(one["system"][1]["text"], "the shared part");
    assert_eq!(one["system"][1]["cache_control"]["type"], "ephemeral");
    assert_eq!(one["messages"][0]["content"][0]["text"], "the function");
    assert_eq!(one["max_tokens"], 16_000);
    assert!(one.get("thinking").is_none());
    // The second try has room for a long answer and is told to think less: still
    // thinking, which a long function needs.
    let (_, two) = &seen[1];
    assert_eq!(two["max_tokens"], 48_000);
    assert_eq!(two["output_config"]["effort"], "low");
    assert!(two.get("thinking").is_none());
    assert_eq!(two["system"], one["system"]);
}

#[test]
fn thinking_is_turned_off_only_after_less_of_it_ran_out_of_room_too() {
    let thought = || {
        (
            200,
            "",
            serde_json::json!({
                "stop_reason": "max_tokens",
                "content": [{ "type": "thinking", "thinking": "", "signature": "s" }],
                "usage": { "input_tokens": 1_000, "output_tokens": 500 },
            }),
        )
    };
    let (base, seen) = server(vec![thought(), thought(), thought()]);
    let claude = Anthropic::with_key("test", None, None).at(&base);
    let reply = claude.complete("instructions", "", "the function").unwrap();
    let why = reply.cut_off.expect("it says nothing was written");
    assert!(why.contains("all of it was thinking"), "{why}");
    assert_eq!((reply.input_tokens, reply.output_tokens), (3_000, 1_500));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen[1].1.get("thinking").is_none());
    assert_eq!(seen[1].1["output_config"]["effort"], "low");
    assert_eq!(seen[2].1["thinking"]["type"], "disabled");
}

#[test]
fn an_answer_cut_off_every_time_is_said_with_what_it_cost() {
    let cut = || {
        (
            200,
            "",
            serde_json::json!({
                "stop_reason": "max_tokens",
                "content": [{ "type": "text", "text": "fn a() {\n    let x = 1;\n" }],
                "usage": { "input_tokens": 1_000, "output_tokens": 500 },
            }),
        )
    };
    let (base, seen) = server(vec![cut(), cut(), cut()]);
    let claude = Anthropic::with_key("test", None, None).at(&base);
    let reply = claude.complete("instructions", "", "the function").unwrap();
    let why = reply.cut_off.expect("it says the answer was cut off");
    assert!(
        why.contains("cut off") && why.contains("2 lines of answer"),
        "{why}"
    );
    assert_eq!(reply.text, "");
    assert_eq!((reply.input_tokens, reply.output_tokens), (3_000, 1_500));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    // Without a shared part the system prompt is the instructions alone.
    assert_eq!(seen[0].1["system"], "instructions");
    // The third try is told how long it may be.
    let last = seen[2].1["messages"][0]["content"].as_array().unwrap();
    assert!(
        last.last().unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("ran past the length")
    );
}

#[test]
fn claude_counts_a_prompt_without_answering_it() {
    let (base, seen) = server(vec![(
        200,
        "",
        serde_json::json!({ "input_tokens": 86_965 }),
    )]);
    let claude = Anthropic::with_key("test", None, None).at(&base);
    assert_eq!(claude.count("i", "s", "p").unwrap(), Some(86_965));
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, "/v1/messages/count_tokens");
    assert!(seen[0].1.get("max_tokens").is_none());
}

#[test]
fn a_chat_completions_server_is_asked_and_waited_for_when_at_its_limit() {
    let (base, seen) = server(vec![
        (
            429,
            "retry-after: 0\r\n",
            serde_json::json!([{ "error": { "message": "quota: try again" } }]),
        ),
        (
            200,
            "",
            serde_json::json!({
                "choices": [{ "finish_reason": "stop", "message": { "role": "assistant", "content": "```rust\nfn a() {}\n```" } }],
                "usage": { "prompt_tokens": 900, "completion_tokens": 40, "prompt_tokens_details": { "cached_tokens": 600 } },
            }),
        ),
    ]);
    let it = Compatible::at(&format!("{base}/v1/"), Some("some-model"), Some("low")).unwrap();
    assert!(it.name().contains("some-model"));
    let reply = it
        .complete("instructions", "shared", "the function")
        .unwrap();
    assert!(reply.text.contains("fn a()"));
    assert_eq!(
        (reply.input_tokens, reply.output_tokens, reply.cached_tokens),
        (900, 40, 600)
    );
    // What it costs is the service's to say.
    assert_eq!(reply.dollars, None);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    let (path, body) = &seen[1];
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(body["model"], "some-model");
    assert_eq!(body["reasoning_effort"], "low");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "instructions");
    // The shared part first, word for word, so a service that caches a prompt's start can.
    assert_eq!(body["messages"][1]["content"], "shared\n\nthe function");
}

#[test]
fn a_chat_completions_answer_cut_off_is_that_functions_alone() {
    let (base, _) = server(vec![(
        200,
        "",
        serde_json::json!({
            "choices": [{ "finish_reason": "length", "message": { "content": "fn a() {" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 },
        }),
    )]);
    let it = Compatible::at(&base, Some("m"), None).unwrap();
    let reply = it.complete("i", "", "p").unwrap();
    assert!(reply.cut_off.unwrap().contains("cut off"));
    assert_eq!(reply.text, "");
}

#[test]
fn a_service_with_free_models_is_known_by_name() {
    assert_eq!(model::free_service("gemini"), Some("Gemini"));
    assert_eq!(model::free_service("google"), Some("Gemini"));
    assert_eq!(model::free_service("openrouter"), Some("OpenRouter"));
    assert_eq!(model::free_service("anthropic"), None);
}
