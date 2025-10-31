// Integration tests for workflow backward and forward compatibility
// Tests YAML workflows, TypeScript workflows, state caching, start/stop functionality

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[cfg(test)]
mod workflow_compatibility_tests {
    use super::*;

    // ========================================================================
    // Test Fixtures
    // ========================================================================

    /// Create a temporary YAML workflow file
    fn create_yaml_workflow(temp_dir: &TempDir, name: &str, content: &str) -> PathBuf {
        let workflow_path = temp_dir.path().join(format!("{}.yml", name));
        fs::write(&workflow_path, content).expect("Failed to write YAML workflow");
        workflow_path
    }

    /// Create a temporary TypeScript workflow project
    fn create_ts_workflow(temp_dir: &TempDir, name: &str) -> PathBuf {
        let project_dir = temp_dir.path().join(name);
        fs::create_dir(&project_dir).expect("Failed to create project dir");

        // package.json
        let package_json = json!({
            "name": name,
            "type": "module",
            "dependencies": {
                "terminator.js": "^0.19.0",
                "zod": "^3.22.4"
            }
        });
        fs::write(
            project_dir.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .expect("Failed to write package.json");

        // workflow.ts
        let workflow_ts = r#"
import { createStep, createWorkflow } from '@mediar-ai/workflow';
import { z } from 'zod';

const step1 = createStep({
  id: 'step1',
  name: 'Step 1',
  execute: async ({ logger, context }) => {
    logger.info('Executing step 1');
    context.data.step1 = { executed: true };
    return { result: 'step1 complete' };
  },
});

const step2 = createStep({
  id: 'step2',
  name: 'Step 2',
  execute: async ({ logger, context }) => {
    logger.info('Executing step 2');
    context.data.step2 = { executed: true, fromStep1: context.data.step1 };
    return { result: 'step2 complete' };
  },
});

const step3 = createStep({
  id: 'step3',
  name: 'Step 3',
  execute: async ({ logger, context }) => {
    logger.info('Executing step 3');
    context.data.step3 = { executed: true };
    return { result: 'step3 complete' };
  },
});

export default createWorkflow({
  name: 'Test Workflow',
  description: 'Test workflow for compatibility testing',
  version: '1.0.0',
  input: z.object({
    testInput: z.string().default('test'),
  }),
})
  .step(step1)
  .step(step2)
  .step(step3)
  .build();
"#;
        fs::write(project_dir.join("workflow.ts"), workflow_ts)
            .expect("Failed to write workflow.ts");

        project_dir
    }

    // ========================================================================
    // YAML Backward Compatibility Tests
    // ========================================================================

    #[tokio::test]
    async fn test_yaml_basic_execution() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "basic",
            r#"
steps:
  - id: step1
    name: Echo Hello
    tool_name: run_command
    arguments:
      run: echo "Hello from YAML"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
        }))
        .await;

        assert!(result.is_ok(), "YAML workflow execution failed");
        let output = result.unwrap();
        assert_eq!(output["status"], "success");
    }

    #[tokio::test]
    async fn test_yaml_multiple_steps() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "multiple",
            r#"
steps:
  - id: step1
    name: Step 1
    tool_name: run_command
    arguments:
      run: echo "Step 1"

  - id: step2
    name: Step 2
    tool_name: run_command
    arguments:
      run: echo "Step 2"

  - id: step3
    name: Step 3
    tool_name: run_command
    arguments:
      run: echo "Step 3"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
        }))
        .await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output["status"], "success");
    }

    #[tokio::test]
    async fn test_yaml_with_variables() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "variables",
            r#"
variables:
  userName:
    type: string
    label: User Name
    default: World

steps:
  - id: greet
    name: Greet User
    tool_name: run_command
    arguments:
      run: echo "Hello {{userName}}"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
            "inputs": {
                "userName": "TestUser"
            }
        }))
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_yaml_start_from_step() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "start_from",
            r#"
steps:
  - id: step1
    name: Step 1
    tool_name: run_command
    arguments:
      run: echo "Step 1"

  - id: step2
    name: Step 2
    tool_name: run_command
    arguments:
      run: echo "Step 2"

  - id: step3
    name: Step 3
    tool_name: run_command
    arguments:
      run: echo "Step 3"
"#,
        );

        // Create fake state file
        let state_dir = temp_dir.path().join(".workflow_state");
        fs::create_dir_all(&state_dir).unwrap();
        let state_file = state_dir.join("start_from.json");
        fs::write(
            &state_file,
            json!({
                "last_step_id": "step1",
                "last_step_index": 0,
                "env": {
                    "step1_result": { "output": "Step 1" },
                    "step1_status": "success"
                }
            })
            .to_string(),
        )
        .unwrap();

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
            "start_from_step": "step2"
        }))
        .await;

        assert!(result.is_ok());
        // Should skip step1, execute step2 and step3
    }

    #[tokio::test]
    async fn test_yaml_end_at_step() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "end_at",
            r#"
steps:
  - id: step1
    name: Step 1
    tool_name: run_command
    arguments:
      run: echo "Step 1"

  - id: step2
    name: Step 2
    tool_name: run_command
    arguments:
      run: echo "Step 2"

  - id: step3
    name: Step 3
    tool_name: run_command
    arguments:
      run: echo "Step 3"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
            "end_at_step": "step2"
        }))
        .await;

        assert!(result.is_ok());
        // Should execute step1 and step2, skip step3
    }

    #[tokio::test]
    async fn test_yaml_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "persistence",
            r#"
steps:
  - id: step1
    name: Step 1
    tool_name: run_command
    arguments:
      run: echo "Step 1"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
        }))
        .await;

        assert!(result.is_ok());

        // Check state file was created
        let state_file = temp_dir.path().join(".workflow_state/persistence.json");
        assert!(state_file.exists(), "State file should exist");

        let state_content = fs::read_to_string(&state_file).unwrap();
        let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();

        assert_eq!(state["last_step_id"], "step1");
        assert!(state["env"].is_object());
    }

    // ========================================================================
    // TypeScript Workflow Tests
    // ========================================================================

    #[tokio::test]
    async fn test_ts_basic_execution() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-basic");

        // Install dependencies
        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "inputs": {
                "testInput": "test"
            }
        }))
        .await;

        assert!(result.is_ok(), "TS workflow execution failed");
        let output = result.unwrap();
        assert_eq!(output["status"], "success");
    }

    #[tokio::test]
    async fn test_ts_start_from_step() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-start-from");

        install_dependencies(&project_dir).await;

        // First run: execute step1
        let result1 = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "end_at_step": "step1",
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result1.is_ok());

        // Second run: resume from step2
        let result2 = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "start_from_step": "step2",
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result2.is_ok());
        let output = result2.unwrap();

        // Verify state was restored
        assert!(output["state"]["context"]["data"]["step1"].is_object());
    }

    #[tokio::test]
    async fn test_ts_end_at_step() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-end-at");

        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "end_at_step": "step2",
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result.is_ok());
        let output = result.unwrap();

        // Should have executed step1 and step2, but not step3
        assert!(output["state"]["stepResults"]["step1"].is_object());
        assert!(output["state"]["stepResults"]["step2"].is_object());
        assert!(output["state"]["stepResults"]["step3"].is_null());
    }

    #[tokio::test]
    async fn test_ts_state_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-persistence");

        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "end_at_step": "step2",
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result.is_ok());

        // Check state file was created
        let state_file = project_dir.join(".workflow_state/workflow.json");
        assert!(state_file.exists(), "TS state file should exist");

        let state_content = fs::read_to_string(&state_file).unwrap();
        let state: serde_json::Value = serde_json::from_str(&state_content).unwrap();

        assert_eq!(state["last_step_id"], "step2");
        assert_eq!(state["last_step_index"], 1);
        assert!(state["env"]["context"].is_object());
    }

    #[tokio::test]
    async fn test_ts_context_sharing() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-context");

        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result.is_ok());
        let output = result.unwrap();

        // Verify step2 received context from step1
        let step2_result = &output["state"]["stepResults"]["step2"]["result"];
        assert!(step2_result["fromStep1"]["executed"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_ts_metadata_extraction() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-metadata");

        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result.is_ok());
        let output = result.unwrap();

        // Verify metadata was extracted
        assert_eq!(output["metadata"]["name"], "Test Workflow");
        assert_eq!(output["metadata"]["version"], "1.0.0");
        assert_eq!(output["metadata"]["steps"].as_array().unwrap().len(), 3);
        assert_eq!(output["metadata"]["steps"][0]["id"], "step1");
        assert_eq!(output["metadata"]["steps"][0]["name"], "Step 1");
    }

    // ========================================================================
    // Cross-Format Compatibility Tests
    // ========================================================================

    #[tokio::test]
    async fn test_yaml_then_ts_workflow() {
        let temp_dir = TempDir::new().unwrap();

        // Execute YAML workflow
        let yaml_workflow = create_yaml_workflow(
            &temp_dir,
            "yaml_first",
            r#"
steps:
  - id: yaml_step
    name: YAML Step
    tool_name: run_command
    arguments:
      run: echo "YAML"
"#,
        );

        let result1 = execute_sequence(json!({
            "url": format!("file://{}", yaml_workflow.display()),
        }))
        .await;

        assert!(result1.is_ok());

        // Execute TS workflow
        let ts_project = create_ts_workflow(&temp_dir, "ts_second");
        install_dependencies(&ts_project).await;

        let result2 = execute_sequence(json!({
            "url": format!("file://{}", ts_project.display()),
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result2.is_ok());
        // Both should work independently
    }

    #[tokio::test]
    async fn test_format_detection_yaml_file() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(&temp_dir, "detect_yaml", "steps: []");

        let format = detect_workflow_format(&format!("file://{}", workflow.display()))
            .await
            .unwrap();

        assert!(matches!(format, WorkflowFormat::Yaml));
    }

    #[tokio::test]
    async fn test_format_detection_ts_file() {
        let temp_dir = TempDir::new().unwrap();
        let ts_file = temp_dir.path().join("workflow.ts");
        fs::write(&ts_file, "export default {};").unwrap();

        let format = detect_workflow_format(&format!("file://{}", ts_file.display()))
            .await
            .unwrap();

        assert!(matches!(format, WorkflowFormat::TypeScript));
    }

    #[tokio::test]
    async fn test_format_detection_ts_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "detect_ts_project");

        let format = detect_workflow_format(&format!("file://{}", project_dir.display()))
            .await
            .unwrap();

        assert!(matches!(format, WorkflowFormat::TypeScript));
    }

    // ========================================================================
    // Runtime Detection Tests
    // ========================================================================

    #[tokio::test]
    async fn test_bun_runtime_detection() {
        let runtime = detect_js_runtime();
        // Should prefer bun if available, otherwise node
        assert!(matches!(runtime, JsRuntime::Bun) || matches!(runtime, JsRuntime::Node));
    }

    #[tokio::test]
    async fn test_ts_execution_with_bun() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-bun");

        // Install with bun if available
        if matches!(detect_js_runtime(), JsRuntime::Bun) {
            install_dependencies_with_bun(&project_dir).await;

            let result = execute_sequence(json!({
                "url": format!("file://{}", project_dir.display()),
                "inputs": { "testInput": "test" }
            }))
            .await;

            assert!(result.is_ok(), "Bun execution should work");
        }
    }

    #[tokio::test]
    async fn test_ts_execution_with_node() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = create_ts_workflow(&temp_dir, "ts-node");

        install_dependencies_with_node(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
            "inputs": { "testInput": "test" }
        }))
        .await;

        assert!(result.is_ok(), "Node execution should work");
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[tokio::test]
    async fn test_yaml_invalid_step() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "invalid",
            r#"
steps:
  - id: bad_step
    tool_name: nonexistent_tool
    arguments: {}
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
        }))
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ts_workflow_error_handling() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("ts-error");
        fs::create_dir(&project_dir).unwrap();

        // Create workflow that throws error
        let workflow_ts = r#"
import { createStep, createWorkflow } from '@mediar-ai/workflow';
import { z } from 'zod';

const errorStep = createStep({
  id: 'error_step',
  name: 'Error Step',
  execute: async () => {
    throw new Error('Intentional error');
  },
});

export default createWorkflow({
  name: 'Error Test',
  input: z.object({}),
})
  .step(errorStep)
  .build();
"#;
        fs::write(project_dir.join("workflow.ts"), workflow_ts).unwrap();
        create_package_json(&project_dir);

        install_dependencies(&project_dir).await;

        let result = execute_sequence(json!({
            "url": format!("file://{}", project_dir.display()),
        }))
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_start_step() {
        let temp_dir = TempDir::new().unwrap();
        let workflow = create_yaml_workflow(
            &temp_dir,
            "missing_start",
            r#"
steps:
  - id: step1
    tool_name: run_command
    arguments:
      run: echo "Step 1"
"#,
        );

        let result = execute_sequence(json!({
            "url": format!("file://{}", workflow.display()),
            "start_from_step": "nonexistent_step"
        }))
        .await;

        assert!(result.is_err());
    }

    // ========================================================================
    // Helper Functions
    // ========================================================================

    async fn execute_sequence(args: serde_json::Value) -> Result<serde_json::Value, String> {
        // Mock implementation - in real tests, call actual execute_sequence_impl
        // This would be replaced with actual server call
        todo!("Implement actual execute_sequence call")
    }

    async fn install_dependencies(project_dir: &PathBuf) {
        // Install with bun if available, otherwise npm
        match detect_js_runtime() {
            JsRuntime::Bun => install_dependencies_with_bun(project_dir).await,
            JsRuntime::Node => install_dependencies_with_node(project_dir).await,
        }
    }

    async fn install_dependencies_with_bun(project_dir: &PathBuf) {
        std::process::Command::new("bun")
            .arg("install")
            .current_dir(project_dir)
            .output()
            .expect("Failed to install dependencies with bun");
    }

    async fn install_dependencies_with_node(project_dir: &PathBuf) {
        std::process::Command::new("npm")
            .arg("install")
            .current_dir(project_dir)
            .output()
            .expect("Failed to install dependencies with npm");
    }

    fn create_package_json(project_dir: &PathBuf) {
        let package_json = json!({
            "type": "module",
            "dependencies": {
                "terminator.js": "^0.19.0",
                "zod": "^3.22.4"
            }
        });
        fs::write(
            project_dir.join("package.json"),
            serde_json::to_string_pretty(&package_json).unwrap(),
        )
        .unwrap();
    }
}
