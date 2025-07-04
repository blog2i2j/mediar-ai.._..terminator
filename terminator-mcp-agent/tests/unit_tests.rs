use serde_json::json;
use terminator_mcp_agent::utils::{ExecuteSequenceArgs, SequenceStep, ToolCall};

#[test]
fn test_execute_sequence_args_serialization() {
    let args = ExecuteSequenceArgs {
        items: vec![SequenceStep {
            tool_name: Some("click_element".to_string()),
            arguments: Some(json!({
                "selector": "button|Submit"
            })),
            continue_on_error: Some(true),
            delay_ms: Some(100),
            group_name: None,
            steps: None,
            skippable: None,
        }],
        stop_on_error: Some(false),
        include_detailed_results: Some(true),
    };

    let json = serde_json::to_string(&args).unwrap();
    assert!(json.contains("items"));
    assert!(json.contains("click_element"));
}

#[test]
fn test_execute_sequence_args_deserialization() {
    let json = r#"{
        "items": [{
            "tool_name": "another_tool",
            "arguments": {"foo": "bar"},
            "continue_on_error": false,
            "delay_ms": 200
        }],
        "stop_on_error": true,
        "include_detailed_results": false
    }"#;

    let deserialized: ExecuteSequenceArgs = serde_json::from_str(json).unwrap();

    // Verify the items content
    assert_eq!(deserialized.items.len(), 1);
    assert_eq!(
        deserialized.items[0].tool_name,
        Some("another_tool".to_string())
    );
    assert_eq!(
        deserialized.items[0].arguments.as_ref().unwrap()["foo"],
        "bar"
    );
    assert_eq!(deserialized.items[0].continue_on_error, Some(false));
    assert_eq!(deserialized.items[0].delay_ms, Some(200));

    assert_eq!(deserialized.stop_on_error, Some(true));
    assert_eq!(deserialized.include_detailed_results, Some(false));
}

#[test]
fn test_execute_sequence_args_default_values() {
    let json = r#"{
        "items": []
    }"#;

    let args: ExecuteSequenceArgs = serde_json::from_str(json).unwrap();

    // Verify it's an empty array
    assert_eq!(args.items.len(), 0);

    assert_eq!(args.stop_on_error, None);
    assert_eq!(args.include_detailed_results, None);
}

#[test]
fn test_tool_call_defaults() {
    // Test that optional fields can be omitted
    let json_str = r#"{
        "tool_name": "minimal_tool",
        "arguments": {}
    }"#;

    let tool_call: ToolCall = serde_json::from_str(json_str).unwrap();
    assert_eq!(tool_call.tool_name, "minimal_tool");
    assert_eq!(tool_call.arguments, json!({}));
    assert_eq!(tool_call.continue_on_error, None);
    assert_eq!(tool_call.delay_ms, None);
}

#[test]
fn test_execute_sequence_minimal() {
    // Test minimal valid execute sequence args
    let json_str = r#"{
        "items": []
    }"#;

    let args: ExecuteSequenceArgs = serde_json::from_str(json_str).unwrap();
    assert_eq!(args.items.len(), 0);
    assert_eq!(args.stop_on_error, None);
    assert_eq!(args.include_detailed_results, None);
}

#[test]
fn test_complex_arguments_preservation() {
    let complex_args = json!({
        "nested": {
            "array": [1, 2, 3],
            "object": {
                "key": "value"
            }
        },
        "boolean": true,
        "number": 42.5,
        "null_value": null
    });

    let tool_call = ToolCall {
        tool_name: "complex_tool".to_string(),
        arguments: complex_args.clone(),
        continue_on_error: None,
        delay_ms: None,
    };

    let serialized = serde_json::to_value(&tool_call).unwrap();
    assert_eq!(serialized["arguments"], complex_args);
}

#[test]
fn test_sequence_step_with_group() {
    // Test that SequenceStep can handle grouped steps
    let json_str = r#"{
        "group_name": "test_group",
        "steps": [{
            "tool_name": "tool1",
            "arguments": {"param": "value"}
        }],
        "skippable": true
    }"#;

    let step: SequenceStep = serde_json::from_str(json_str).unwrap();
    assert_eq!(step.group_name, Some("test_group".to_string()));
    assert_eq!(step.skippable, Some(true));
    assert!(step.steps.is_some());
    assert_eq!(step.steps.as_ref().unwrap().len(), 1);
    assert_eq!(step.steps.as_ref().unwrap()[0].tool_name, "tool1");
}
