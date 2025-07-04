use rmcp::handler::server::tool::Parameters;
use serde_json::json;
use terminator_mcp_agent::server::DesktopWrapper;
use terminator_mcp_agent::utils::{
    ExecuteSequenceArgs, SequenceStep, ToolCall, ValidateElementArgs,
};

/// Helper function to extract JSON from tool response
fn extract_json_from_content(content: &[rmcp::model::Content]) -> Option<serde_json::Value> {
    content.first().and_then(|c| {
        // Serialize the content to JSON to extract the text field
        let content_json = serde_json::to_value(c).ok()?;
        content_json
            .get("text")
            .and_then(|t| t.as_str())
            .and_then(|text| serde_json::from_str(text).ok())
    })
}

#[tokio::test]
async fn test_execute_sequence_direct() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // Test execute_sequence with empty tools array
    let args = ExecuteSequenceArgs {
        items: vec![],
        stop_on_error: Some(true),
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await;
    assert!(result.is_ok(), "execute_sequence failed");

    let call_result = result.unwrap();
    assert!(!call_result.content.is_empty(), "No content returned");

    // Verify the response
    let parsed = extract_json_from_content(&call_result.content)
        .expect("Failed to extract JSON from response");

    assert_eq!(parsed["action"], "execute_sequence");
    assert_eq!(parsed["status"], "success");
    assert_eq!(parsed["total_tools"], 0);
    assert_eq!(parsed["executed_tools"], 0);
}

#[tokio::test]
async fn test_validate_element_direct() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // Test validate_element with a non-existent element
    let args = ValidateElementArgs {
        retries: Some(0),
        selector: "#99999999999".to_string(),
        alternative_selectors: None,
        timeout_ms: Some(100),
        include_tree: Some(false),
    };

    let result = desktop.validate_element(Parameters(args)).await;
    assert!(
        result.is_ok(),
        "validate_element should handle not found gracefully"
    );

    let call_result = result.unwrap();
    let parsed = extract_json_from_content(&call_result.content)
        .expect("Failed to extract JSON from response");

    assert_eq!(parsed["action"], "validate_element");
    assert_eq!(parsed["status"], "failed");
    assert_eq!(parsed["exists"], false);
}

#[tokio::test]
async fn test_execute_sequence_with_invalid_tool_stops() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // Test execute_sequence with an invalid tool and stop_on_error: true
    let args = ExecuteSequenceArgs {
        items: vec![SequenceStep {
            tool_name: Some("non_existent_tool".to_string()),
            arguments: Some(json!({})),
            continue_on_error: Some(false), // Explicitly do not continue
            delay_ms: None,
            group_name: None,
            steps: None,
            skippable: None,
        }],
        stop_on_error: Some(true),
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await;
    assert!(
        result.is_ok(),
        "execute_sequence should handle invalid tools gracefully"
    );

    let call_result = result.unwrap();
    let parsed = extract_json_from_content(&call_result.content)
        .expect("Failed to extract JSON from response");

    assert_eq!(parsed["action"], "execute_sequence");
    assert_eq!(
        parsed["status"], "partial_success",
        "Status should be partial_success as it stopped on error"
    );
    assert_eq!(parsed["total_tools"], 1);
    assert_eq!(parsed["executed_tools"], 1);

    // Check that the error was captured
    let results = parsed["results"]
        .as_array()
        .expect("Expected results array");
    let first_result = &results[0];
    assert_eq!(first_result["status"], "error");
    assert!(first_result["error"]
        .as_str()
        .unwrap()
        .contains("Unknown tool"));
}

#[tokio::test]
async fn test_continue_on_error_allows_sequence_to_proceed() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // This sequence should complete, as the first tool's failure is ignored.
    let args = ExecuteSequenceArgs {
        items: vec![
            SequenceStep {
                tool_name: Some("invalid_tool".to_string()),
                arguments: Some(json!({})),
                continue_on_error: Some(true), // This should be respected
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({
                    "selector": "#test-element",
                    "timeout_ms": 10
                })),
                continue_on_error: None,
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
        ],
        stop_on_error: Some(false), // Sequence-level stop is false
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await;
    assert!(result.is_ok(), "execute_sequence failed");

    let call_result = result.unwrap();
    let parsed = extract_json_from_content(&call_result.content)
        .expect("Failed to extract JSON from response");

    assert_eq!(parsed["status"], "completed_with_errors");
    assert_eq!(parsed["total_tools"], 2);
    assert_eq!(
        parsed["executed_tools"], 2,
        "Both tools should have been executed"
    );

    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);

    // First tool should be marked as skipped
    assert_eq!(results[0]["status"], "skipped");
    // Second tool should have succeeded
    assert_eq!(results[1]["status"], "success");
}

#[tokio::test]
async fn test_execute_sequence_delays() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // Test that delays are respected
    let start_time = std::time::Instant::now();

    let args = ExecuteSequenceArgs {
        items: vec![
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({
                    "selector": "#test-delay-1",
                    "timeout_ms": 50
                })),
                continue_on_error: None,
                delay_ms: Some(100),
                group_name: None,
                steps: None,
                skippable: None,
            },
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({
                    "selector": "#test-delay-2",
                    "timeout_ms": 50
                })),
                continue_on_error: None,
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
        ],
        stop_on_error: Some(true),
        include_detailed_results: Some(false),
    };

    let result = desktop.execute_sequence(Parameters(args)).await;
    assert!(result.is_ok());

    let elapsed = start_time.elapsed();
    // Should have at least 100ms delay after first tool
    assert!(
        elapsed.as_millis() >= 100,
        "Delays not properly applied: {}ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_sequence_with_skippable_failing_group() {
    let desktop = DesktopWrapper::new().await.unwrap();

    let args = ExecuteSequenceArgs {
        items: vec![
            // A skippable group with a failing tool
            SequenceStep {
                group_name: Some("Skippable Group".to_string()),
                skippable: Some(true),
                steps: Some(vec![ToolCall {
                    tool_name: "non_existent_tool".to_string(),
                    arguments: json!({}),
                    continue_on_error: Some(false), // This should not stop the sequence
                    delay_ms: None,
                }]),
                tool_name: None,
                arguments: None,
                continue_on_error: None,
                delay_ms: None,
            },
            // A regular successful tool that should be executed
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({"selector": "#some-element", "timeout_ms": 10})),
                continue_on_error: None,
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
        ],
        stop_on_error: Some(true), // stop_on_error is true, but the failing group is skippable
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await.unwrap();
    let parsed = extract_json_from_content(&result.content).unwrap();

    assert_eq!(
        parsed["status"], "completed_with_errors",
        "Sequence should complete with errors due to the failing skippable group. Full response: {:?}",
        parsed
    );
    assert_eq!(parsed["executed_tools"], 2);

    let results = parsed["results"].as_array().unwrap();
    // Check skippable group result
    assert_eq!(results[0]["group_name"], "Skippable Group");
    assert_eq!(results[0]["status"], "partial_success");
    // Check successful tool result
    assert_eq!(results[1]["tool_name"], "validate_element");
    assert_eq!(results[1]["status"], "success");
}

#[tokio::test]
async fn test_sequence_with_unskippable_failing_group_stops() {
    let desktop = DesktopWrapper::new().await.unwrap();

    let args = ExecuteSequenceArgs {
        items: vec![
            // An unskippable group with a failing tool
            SequenceStep {
                group_name: Some("Unskippable Group".to_string()),
                skippable: Some(false), // Explicitly not skippable
                steps: Some(vec![ToolCall {
                    tool_name: "non_existent_tool".to_string(),
                    arguments: json!({}),
                    continue_on_error: Some(false),
                    delay_ms: None,
                }]),
                tool_name: None,
                arguments: None,
                continue_on_error: None,
                delay_ms: None,
            },
            // This tool should NOT be executed
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({"selector": "#should-not-run"})),
                continue_on_error: None,
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
        ],
        stop_on_error: Some(true),
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await.unwrap();
    let parsed = extract_json_from_content(&result.content).unwrap();

    assert_eq!(
        parsed["status"], "partial_success",
        "Sequence should stop and report partial success."
    );
    assert_eq!(parsed["total_tools"], 2);
    assert_eq!(
        parsed["executed_tools"], 1,
        "Sequence should have stopped after the first failing group"
    );

    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results[0]["group_name"], "Unskippable Group");
    assert_eq!(results[0]["status"], "partial_success");
}

#[tokio::test]
async fn test_stop_on_error_halts_sequence() {
    let desktop = match DesktopWrapper::new().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Skipping test due to Desktop initialization failure: {}", e);
            return;
        }
    };

    // This sequence should stop after the first tool fails because stop_on_error is true
    // and the tool is not in a skippable group.
    let args = ExecuteSequenceArgs {
        items: vec![
            SequenceStep {
                tool_name: Some("invalid_tool".to_string()),
                arguments: Some(json!({})),
                continue_on_error: Some(false), // Explicitly false
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
            SequenceStep {
                tool_name: Some("validate_element".to_string()),
                arguments: Some(json!({
                    "selector": "#should-not-be-reached",
                    "timeout_ms": 10
                })),
                continue_on_error: None,
                delay_ms: None,
                group_name: None,
                steps: None,
                skippable: None,
            },
        ],
        stop_on_error: Some(true),
        include_detailed_results: Some(true),
    };

    let result = desktop.execute_sequence(Parameters(args)).await;
    assert!(result.is_ok(), "execute_sequence failed");

    let call_result = result.unwrap();
    let parsed = extract_json_from_content(&call_result.content)
        .expect("Failed to extract JSON from response");

    // Only the first tool should have been executed.
    assert_eq!(parsed["status"], "partial_success");
    assert_eq!(parsed["total_tools"], 2);
    assert_eq!(
        parsed["executed_tools"], 1,
        "Sequence should have stopped after the first error"
    );

    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "error");
}
