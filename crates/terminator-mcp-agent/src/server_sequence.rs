use crate::helpers::substitute_variables;
use crate::output_parser;
use crate::server::extract_content_json;
use crate::telemetry::{StepSpan, WorkflowSpan};
use crate::utils::{
    DesktopWrapper, ExecuteSequenceArgs, SequenceItem, ToolCall, ToolGroup, VariableDefinition,
};
use crate::workflow_format::{detect_workflow_format, WorkflowFormat};
use crate::workflow_typescript::TypeScriptWorkflow;
use rmcp::model::{CallToolResult, Content};
use rmcp::service::{Peer, RequestContext, RoleServer};
use rmcp::ErrorData as McpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Helper function to recursively validate a value against a variable definition
fn validate_variable_value(
    variable_name: &str,
    value: &Value,
    def: &VariableDefinition,
) -> Result<(), McpError> {
    match def.r#type {
        crate::utils::VariableType::String => {
            if !value.is_string() {
                return Err(McpError::invalid_params(
                    format!("Variable '{variable_name}' must be a string."),
                    Some(json!({"value": value})),
                ));
            }
        }
        crate::utils::VariableType::Number => {
            if !value.is_number() {
                return Err(McpError::invalid_params(
                    format!("Variable '{variable_name}' must be a number."),
                    Some(json!({"value": value})),
                ));
            }
        }
        crate::utils::VariableType::Boolean => {
            if !value.is_boolean() {
                return Err(McpError::invalid_params(
                    format!("Variable '{variable_name}' must be a boolean."),
                    Some(json!({"value": value})),
                ));
            }
        }
        crate::utils::VariableType::Enum => {
            let val_str = value.as_str().ok_or_else(|| {
                McpError::invalid_params(
                    format!("Enum variable '{variable_name}' must be a string."),
                    Some(json!({"value": value})),
                )
            })?;
            if let Some(options) = &def.options {
                if !options.contains(&val_str.to_string()) {
                    return Err(McpError::invalid_params(
                        format!("Variable '{variable_name}' has an invalid value."),
                        Some(json!({
                            "value": val_str,
                            "allowed_options": options
                        })),
                    ));
                }
            }
        }
        crate::utils::VariableType::Array => {
            if !value.is_array() {
                return Err(McpError::invalid_params(
                    format!("Variable '{variable_name}' must be an array."),
                    Some(json!({"value": value})),
                ));
            }
            // Validate each array item against item_schema if provided
            if let Some(item_schema) = &def.item_schema {
                if let Some(array) = value.as_array() {
                    for (index, item) in array.iter().enumerate() {
                        validate_variable_value(
                            &format!("{variable_name}[{index}]"),
                            item,
                            item_schema,
                        )?;
                    }
                }
            }
        }
        crate::utils::VariableType::Object => {
            if !value.is_object() {
                return Err(McpError::invalid_params(
                    format!("Variable '{variable_name}' must be an object."),
                    Some(json!({"value": value})),
                ));
            }

            let obj = value.as_object().unwrap();

            // Validate against properties if defined (for objects with known structure)
            if let Some(properties) = &def.properties {
                for (prop_key, prop_def) in properties {
                    if let Some(prop_value) = obj.get(prop_key) {
                        validate_variable_value(
                            &format!("{variable_name}.{prop_key}"),
                            prop_value,
                            prop_def,
                        )?;
                    } else if prop_def.required.unwrap_or(true) {
                        return Err(McpError::invalid_params(
                            format!("Required property '{variable_name}.{prop_key}' is missing."),
                            None,
                        ));
                    }
                }
            }

            // Validate against value_schema if defined (for flat key-value objects)
            if let Some(value_schema) = &def.value_schema {
                for (key, val) in obj {
                    validate_variable_value(&format!("{variable_name}.{key}"), val, value_schema)?;
                }
            }
        }
    }

    Ok(())
}

impl DesktopWrapper {
    // Get the state file path for a workflow
    async fn get_state_file_path(workflow_url: &str) -> Option<PathBuf> {
        if let Some(file_path) = workflow_url.strip_prefix("file://") {
            let workflow_path = Path::new(file_path);
            let workflow_dir = workflow_path.parent()?;
            let workflow_name = workflow_path.file_stem()?;

            let state_file = workflow_dir
                .join(".workflow_state")
                .join(format!("{}.json", workflow_name.to_string_lossy()));

            Some(state_file)
        } else {
            None
        }
    }

    // Save env state after any step that modifies it
    async fn save_workflow_state(
        workflow_url: &str,
        step_id: Option<&str>,
        step_index: usize,
        env: &serde_json::Value,
    ) -> Result<(), McpError> {
        if let Some(state_file) = Self::get_state_file_path(workflow_url).await {
            if let Some(state_dir) = state_file.parent() {
                tokio::fs::create_dir_all(state_dir).await.map_err(|e| {
                    McpError::internal_error(format!("Failed to create state directory: {e}"), None)
                })?;
            }

            let state = json!({
                "last_updated": chrono::Utc::now().to_rfc3339(),
                "last_step_id": step_id,
                "last_step_index": step_index,
                "workflow_file": Path::new(workflow_url.strip_prefix("file://").unwrap_or(workflow_url))
                    .file_name()
                    .and_then(|n| n.to_str()),
                "env": env,
            });

            tokio::fs::write(
                &state_file,
                serde_json::to_string_pretty(&state).map_err(|e| {
                    McpError::internal_error(format!("Failed to serialize state: {e}"), None)
                })?,
            )
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to write state file: {e}"), None)
            })?;

            debug!("Saved workflow state to: {:?}", state_file);
        }
        Ok(())
    }

    // Load env state when starting from a specific step
    async fn load_workflow_state(
        workflow_url: &str,
    ) -> Result<Option<serde_json::Value>, McpError> {
        if let Some(state_file) = Self::get_state_file_path(workflow_url).await {
            if state_file.exists() {
                let content = tokio::fs::read_to_string(&state_file).await.map_err(|e| {
                    McpError::internal_error(format!("Failed to read state file: {e}"), None)
                })?;
                let state: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                    McpError::internal_error(format!("Failed to parse state file: {e}"), None)
                })?;

                if let Some(env) = state.get("env") {
                    debug!(
                        "Loaded workflow state from step {} ({})",
                        state["last_step_index"],
                        state["last_step_id"].as_str().unwrap_or("unknown")
                    );
                    return Ok(Some(env.clone()));
                }
            } else {
                debug!("No saved workflow state found at: {:?}", state_file);
            }
        }
        Ok(None)
    }

    /// Helper function to create a flattened execution context where env properties
    /// are available both under 'env.' prefix and directly at the top level.
    /// This enables conditions to access env variables directly without the 'env.' prefix,
    /// matching the behavior of script execution.
    fn create_flattened_execution_context(
        execution_context_map: &serde_json::Map<String, serde_json::Value>,
    ) -> serde_json::Value {
        let mut flattened_map = execution_context_map.clone();

        // Flatten env properties to top level
        if let Some(env_value) = flattened_map.get("env") {
            if let Some(env_obj) = env_value.as_object() {
                // Clone env properties to avoid borrow issues
                let env_entries: Vec<(String, serde_json::Value)> = env_obj
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                // Insert each env property at top level
                // Note: env properties will override existing top-level keys with same name
                for (key, value) in env_entries {
                    flattened_map.insert(key, value);
                }
            }
        }

        serde_json::Value::Object(flattened_map)
    }

    /// Deep merge JSON values - recursively merges objects, overwrites other types
    /// This matches the Python executor's deep_merge behavior:
    /// - For objects: recursively merge keys from source into target
    /// - For other types: source value overwrites target value
    fn deep_merge_json(target: &mut serde_json::Map<String, Value>, source: &Value) {
        if let Some(source_obj) = source.as_object() {
            for (key, source_value) in source_obj {
                if let Some(target_value) = target.get_mut(key) {
                    // Key exists in target
                    if target_value.is_object() && source_value.is_object() {
                        // Both are objects - recursively merge
                        if let Some(target_obj) = target_value.as_object_mut() {
                            Self::deep_merge_json(target_obj, source_value);
                        }
                    } else {
                        // Not both objects - source overwrites target
                        *target_value = source_value.clone();
                    }
                } else {
                    // Key doesn't exist in target - add it
                    target.insert(key.clone(), source_value.clone());
                }
            }
        }
    }

    pub async fn execute_sequence_impl(
        &self,
        peer: Peer<RoleServer>,
        request_context: RequestContext<RoleServer>,
        mut args: ExecuteSequenceArgs,
    ) -> Result<CallToolResult, McpError> {
        // Validate that either URL or steps are provided
        if args.url.is_none() && args.steps.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            return Err(McpError::invalid_params(
                "Either 'url' or 'steps' must be provided".to_string(),
                None,
            ));
        }

        // Detect workflow format if URL is provided
        if let Some(url) = &args.url {
            let format = detect_workflow_format(url);

            match format {
                WorkflowFormat::TypeScript => {
                    // Execute TypeScript workflow
                    let url_clone = url.clone();
                    return self.execute_typescript_workflow(&url_clone, args).await;
                }
                WorkflowFormat::Yaml => {
                    // Continue with existing YAML workflow logic
                    info!("Detected YAML workflow format");
                }
            }
        }

        // Handle URL fetching if provided (YAML workflow path)
        if let Some(url) = &args.url {
            info!("Fetching workflow from URL: {}", url);

            let workflow_content = if url.starts_with("file://") {
                // Handle local file URLs
                let file_path = url.strip_prefix("file://").unwrap_or(url);
                info!("Reading file from path: {}", file_path);

                // Store the workflow directory for relative path resolution
                let workflow_path = Path::new(file_path);
                if let Some(parent_dir) = workflow_path.parent() {
                    let mut workflow_dir_guard = self.current_workflow_dir.lock().await;
                    *workflow_dir_guard = Some(parent_dir.to_path_buf());
                    info!("Stored workflow directory: {:?}", parent_dir);
                }

                let content = std::fs::read_to_string(file_path).map_err(|e| {
                    McpError::invalid_params(
                        format!("Failed to read local workflow file: {e}"),
                        Some(json!({"url": url, "error": e.to_string()})),
                    )
                })?;
                info!("File content length: {}", content.len());
                content
            } else if url.starts_with("http://") || url.starts_with("https://") {
                // Handle HTTP/HTTPS URLs
                let response = reqwest::get(url).await.map_err(|e| {
                    McpError::invalid_params(
                        format!("Failed to fetch workflow from URL: {e}"),
                        Some(json!({"url": url, "error": e.to_string()})),
                    )
                })?;

                if !response.status().is_success() {
                    return Err(McpError::invalid_params(
                        format!("HTTP error fetching workflow: {}", response.status()),
                        Some(json!({"url": url, "status": response.status().as_u16()})),
                    ));
                }

                response.text().await.map_err(|e| {
                    McpError::invalid_params(
                        format!("Failed to read response text: {e}"),
                        Some(json!({"url": url, "error": e.to_string()})),
                    )
                })?
            } else {
                return Err(McpError::invalid_params(
                    "URL must start with http://, https://, or file://".to_string(),
                    Some(json!({"url": url})),
                ));
            };

            // Debug: Log the raw YAML content
            debug!(
                "Raw YAML content (first 500 chars): {}",
                workflow_content.chars().take(500).collect::<String>()
            );

            // Parse the fetched YAML workflow
            // First check if it's wrapped in execute_sequence structure
            let remote_workflow: ExecuteSequenceArgs = if workflow_content
                .contains("tool_name: execute_sequence")
            {
                // This workflow is wrapped in execute_sequence structure
                // Parse as a generic Value first to extract the arguments
                match serde_yaml::from_str::<serde_json::Value>(&workflow_content) {
                    Ok(yaml_value) => {
                        if yaml_value.get("tool_name").and_then(|v| v.as_str())
                            == Some("execute_sequence")
                        {
                            // Extract the arguments field
                            if let Some(arguments) = yaml_value.get("arguments") {
                                match serde_json::from_value::<ExecuteSequenceArgs>(
                                    arguments.clone(),
                                ) {
                                    Ok(wf) => {
                                        info!(
                                            "Successfully parsed wrapped YAML. Steps count: {}",
                                            wf.steps.as_ref().map(|s| s.len()).unwrap_or(0)
                                        );
                                        wf
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "Failed to parse arguments from wrapped YAML: {}",
                                            e
                                        );
                                        return Err(McpError::invalid_params(
                                            format!("Failed to parse workflow arguments: {e}"),
                                            Some(json!({"url": url, "error": e.to_string()})),
                                        ));
                                    }
                                }
                            } else {
                                return Err(McpError::invalid_params(
                                    "Workflow has execute_sequence but no arguments field"
                                        .to_string(),
                                    Some(json!({"url": url})),
                                ));
                            }
                        } else {
                            // Try parsing as regular ExecuteSequenceArgs
                            match serde_json::from_value::<ExecuteSequenceArgs>(yaml_value) {
                                Ok(wf) => {
                                    info!(
                                        "Successfully parsed YAML. Steps count: {}",
                                        wf.steps.as_ref().map(|s| s.len()).unwrap_or(0)
                                    );
                                    wf
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse YAML: {}", e);
                                    return Err(McpError::invalid_params(
                                        format!("Failed to parse workflow YAML: {e}"),
                                        Some(json!({"url": url, "error": e.to_string()})),
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse YAML as Value: {}", e);
                        return Err(McpError::invalid_params(
                            format!("Failed to parse YAML: {e}"),
                            Some(json!({"url": url, "error": e.to_string()})),
                        ));
                    }
                }
            } else {
                // Standard format without execute_sequence wrapper
                match serde_yaml::from_str::<ExecuteSequenceArgs>(&workflow_content) {
                    Ok(wf) => {
                        info!(
                            "Successfully parsed YAML. Steps count: {}",
                            wf.steps.as_ref().map(|s| s.len()).unwrap_or(0)
                        );
                        wf
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse YAML: {}", e);
                        return Err(McpError::invalid_params(
                            format!("Failed to parse remote workflow YAML: {e}"),
                            Some(
                                json!({"url": url, "error": e.to_string(), "content_preview": workflow_content.chars().take(200).collect::<String>()}),
                            ),
                        ));
                    }
                }
            };

            // Debug: Log what we got from the remote workflow
            info!(
                "Remote workflow parsed - steps present: {}, steps count: {}",
                remote_workflow.steps.is_some(),
                remote_workflow.steps.as_ref().map(|s| s.len()).unwrap_or(0)
            );

            // Merge remote workflow with local overrides
            // Only use remote steps if local steps are empty or None
            if args.steps.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                args.steps = remote_workflow.steps;
            }
            // Also merge troubleshooting steps if not provided locally
            if args
                .troubleshooting
                .as_ref()
                .map(|t| t.is_empty())
                .unwrap_or(true)
            {
                args.troubleshooting = remote_workflow.troubleshooting;
            }
            if args.variables.is_none() {
                args.variables = remote_workflow.variables;
            }
            if args.selectors.is_none() {
                args.selectors = remote_workflow.selectors;
            }
            // Merge inputs: local inputs (from CLI) override remote inputs (from workflow file)
            if args.inputs.is_none() && remote_workflow.inputs.is_some() {
                args.inputs = remote_workflow.inputs;
            } else if args.inputs.is_some() && remote_workflow.inputs.is_some() {
                // If both exist, merge them with local taking precedence
                if let (Some(local_inputs), Some(remote_inputs)) =
                    (&args.inputs, &remote_workflow.inputs)
                {
                    if let (Some(local_obj), Some(remote_obj)) =
                        (local_inputs.as_object(), remote_inputs.as_object())
                    {
                        let mut merged = remote_obj.clone();
                        merged.extend(local_obj.clone());
                        args.inputs = Some(serde_json::Value::Object(merged));
                    }
                }
            }

            info!(
                "After merge - args.steps present: {}, count: {}, inputs present: {}",
                args.steps.is_some(),
                args.steps.as_ref().map(|s| s.len()).unwrap_or(0),
                args.inputs.is_some()
            );

            info!(
                "Successfully loaded workflow from URL with {} steps",
                args.steps.as_ref().map(|s| s.len()).unwrap_or(0)
            );

            // Also merge scripts_base_path if not provided locally
            if args.scripts_base_path.is_none() {
                args.scripts_base_path = remote_workflow.scripts_base_path;
            }
            // Also merge output_parser and output if not provided locally
            if args.output_parser.is_none() {
                args.output_parser = remote_workflow.output_parser;
            }
            if args.output.is_none() {
                args.output = remote_workflow.output;
            }
        }

        // Set the scripts_base_path for file resolution in run_command and execute_browser_script
        if let Some(scripts_base_path) = &args.scripts_base_path {
            let mut scripts_base_path_guard = self.current_scripts_base_path.lock().await;
            *scripts_base_path_guard = Some(scripts_base_path.clone());
            info!(
                "[SCRIPTS_BASE_PATH] Setting scripts_base_path for workflow: {}",
                scripts_base_path
            );
            info!(
                "[SCRIPTS_BASE_PATH] Script files will be searched first in: {}",
                scripts_base_path
            );
            info!("[SCRIPTS_BASE_PATH] Fallback search will use workflow directory or current directory");
        } else {
            info!(
                "[SCRIPTS_BASE_PATH] No scripts_base_path specified, using default file resolution"
            );
        }

        // Handle backward compatibility: 'continue' is opposite of 'stop_on_error'
        let stop_on_error = if let Some(continue_exec) = args.r#continue {
            !continue_exec // continue=true means stop_on_error=false
        } else {
            args.stop_on_error.unwrap_or(true)
        };

        // Handle verbosity levels
        // quiet: minimal output (just success/failure)
        // normal: moderate output (includes tool results/logs but may omit some metadata)
        // verbose: full output (includes all details and metadata)
        let include_detailed = match args.verbosity.as_deref() {
            Some("quiet") => false,
            Some("verbose") => true,
            Some("normal") | None => args.include_detailed_results.unwrap_or(false), // Changed default to false
            _ => args.include_detailed_results.unwrap_or(false), // Changed default to false
        };

        // Re-enabling validation logic
        if let Some(variable_schema) = &args.variables {
            let inputs_map = args
                .inputs
                .as_ref()
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            for (key, def) in variable_schema {
                let value = inputs_map.get(key).or(def.default.as_ref());

                match value {
                    Some(val) => {
                        // Use the recursive validation helper function
                        validate_variable_value(key, val, def)?;
                    }
                    None => {
                        if def.required.unwrap_or(true) {
                            return Err(McpError::invalid_params(
                                format!("Required variable '{key}' is missing."),
                                None,
                            ));
                        }
                    }
                }
            }
        }

        // Build the execution context. It's a combination of the 'inputs' and 'selectors'.
        // The context is a simple, flat map of variables that will be used for substitution in tool arguments.
        let mut execution_context_map = serde_json::Map::new();

        // First, populate with default values from variables schema
        if let Some(variable_schema) = &args.variables {
            for (key, def) in variable_schema {
                if let Some(default_value) = &def.default {
                    execution_context_map.insert(key.clone(), default_value.clone());
                }
            }
        }

        // Then override with user-provided inputs (inputs take precedence over defaults)
        if let Some(inputs) = &args.inputs {
            // Validate inputs is an object
            if let Err(err) = crate::utils::validate_inputs(inputs) {
                return Err(McpError::invalid_params(
                    format!(
                        "Invalid inputs: {} expected {}, got {}",
                        err.field, err.expected, err.actual
                    ),
                    None,
                ));
            }
            if let Some(inputs_map) = inputs.as_object() {
                for (key, value) in inputs_map {
                    execution_context_map.insert(key.clone(), value.clone());
                }
            }
        }

        if let Some(selectors) = args.selectors.clone() {
            // Validate selectors
            if let Err(err) = crate::utils::validate_selectors(&selectors) {
                return Err(McpError::invalid_params(
                    format!(
                        "Invalid selectors: {} expected {}, got {}",
                        err.field, err.expected, err.actual
                    ),
                    None,
                ));
            }
            // If selectors is a string, parse it as JSON first
            let selectors_value = if let serde_json::Value::String(s) = &selectors {
                match serde_json::from_str::<serde_json::Value>(s) {
                    Ok(parsed) => parsed,
                    Err(_) => selectors, // If parsing fails, treat it as a raw string
                }
            } else {
                selectors
            };
            execution_context_map.insert("selectors".to_string(), selectors_value);
        }

        // Initialize an internal env bag with the inputs and other values
        let mut env_map = serde_json::Map::new();

        // Add all inputs to the env so they're accessible in JavaScript
        if let Some(inputs) = &args.inputs {
            if let Some(inputs_obj) = inputs.as_object() {
                for (key, value) in inputs_obj {
                    env_map.insert(key.clone(), value.clone());
                }
                // Also store the entire inputs object
                env_map.insert("inputs".to_string(), inputs.clone());
            }
        }

        execution_context_map.insert("env".to_string(), serde_json::Value::Object(env_map));

        // Build a map from step ID to its index for quick lookup (includes both main and troubleshooting steps)
        use std::collections::HashMap;
        let mut id_to_index: HashMap<String, usize> = HashMap::new();

        // Map main workflow steps
        if let Some(steps) = &args.steps {
            for (idx, step) in steps.iter().enumerate() {
                if let Some(id) = &step.id {
                    if id_to_index.insert(id.clone(), idx).is_some() {
                        warn!(
                            "Duplicate step id '{}' found; later occurrence overrides earlier.",
                            id
                        );
                    }
                }
            }
        }

        // Track the boundary between main steps and troubleshooting steps
        let main_steps_len = args.steps.as_ref().map(|s| s.len()).unwrap_or(0);

        // Map troubleshooting steps (they come after main steps in the sequence)
        if let Some(troubleshooting) = &args.troubleshooting {
            for (idx, step) in troubleshooting.iter().enumerate() {
                if let Some(id) = &step.id {
                    let global_idx = main_steps_len + idx;
                    if id_to_index.insert(id.clone(), global_idx).is_some() {
                        warn!(
                            "Duplicate step id '{}' found in troubleshooting; later occurrence overrides earlier.",
                            id
                        );
                    }
                }
            }
        }

        // NEW: Check if we should start from a specific step (now searches both main and troubleshooting)
        let start_from_index = if let Some(start_step) = &args.start_from_step {
            // Find the step index by ID using the complete map
            id_to_index.get(start_step).copied().ok_or_else(|| {
                McpError::invalid_params(
                    format!("start_from_step '{start_step}' not found in workflow or troubleshooting steps"),
                    Some(json!({
                        "requested_step": start_step,
                        "available_steps": id_to_index.keys().cloned().collect::<Vec<_>>()
                    })),
                )
            })?
        } else {
            0
        };

        // NEW: Check if we should end at a specific step (now searches both main and troubleshooting)
        let end_at_index = if let Some(end_step) = &args.end_at_step {
            // Find the step index by ID (inclusive) using the complete map
            id_to_index.get(end_step).copied().ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "end_at_step '{end_step}' not found in workflow or troubleshooting steps"
                    ),
                    Some(json!({
                        "requested_step": end_step,
                        "available_steps": id_to_index.keys().cloned().collect::<Vec<_>>()
                    })),
                )
            })?
        } else {
            // No end_at_step specified, run to the end of MAIN steps only
            // This preserves the default behavior of not entering troubleshooting during normal execution
            main_steps_len.saturating_sub(1)
        };

        // NEW: Load saved state if starting from a specific step
        if start_from_index > 0 {
            if let Some(url) = &args.url {
                if let Some(saved_env) = Self::load_workflow_state(url).await? {
                    execution_context_map.insert("env".to_string(), saved_env);
                    debug!(
                        "Loaded saved env state for resuming from step {}",
                        start_from_index
                    );
                }
            }
        }

        let execution_context = Self::create_flattened_execution_context(&execution_context_map);
        debug!(
            "Executing sequence with context: {}",
            serde_json::to_string_pretty(&execution_context).unwrap_or_default()
        );
        info!(
            "Starting execute_sequence: steps={}, stop_on_error={}, include_detailed_results={}",
            args.steps.as_ref().map(|s| s.len()).unwrap_or(0),
            stop_on_error,
            include_detailed
        );

        // Start workflow telemetry span
        let workflow_name = "execute_sequence";
        let mut workflow_span = WorkflowSpan::new(workflow_name);
        workflow_span.set_attribute(
            "workflow.total_steps",
            args.steps
                .as_ref()
                .map(|s| s.len())
                .unwrap_or(0)
                .to_string(),
        );
        workflow_span.set_attribute("workflow.stop_on_error", stop_on_error.to_string());

        // Convert flattened SequenceStep to internal SequenceItem representation
        let mut sequence_items = Vec::new();
        let empty_steps = Vec::new();
        let steps = args.steps.as_ref().unwrap_or(&empty_steps);
        for step in steps {
            let item = if let Some(tool_name) = &step.tool_name {
                // Parse delay from either delay_ms or human-readable delay field
                let delay_ms = if let Some(delay_str) = &step.delay {
                    match crate::duration_parser::parse_duration(delay_str) {
                        Ok(ms) => Some(ms),
                        Err(e) => {
                            warn!("Failed to parse delay '{}': {}", delay_str, e);
                            step.delay_ms // Fall back to delay_ms
                        }
                    }
                } else {
                    step.delay_ms
                };

                let tool_call = ToolCall {
                    tool_name: tool_name.clone(),
                    arguments: step.arguments.clone().unwrap_or(serde_json::json!({})),
                    continue_on_error: step.continue_on_error,
                    delay_ms,
                    id: step.id.clone(),
                };
                SequenceItem::Tool { tool_call }
            } else if let Some(group_name) = &step.group_name {
                let tool_group = ToolGroup {
                    group_name: group_name.clone(),
                    steps: step
                        .steps
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| ToolCall {
                            tool_name: s.tool_name,
                            arguments: s.arguments,
                            continue_on_error: s.continue_on_error,
                            delay_ms: s.delay_ms,
                            id: s.id,
                        })
                        .collect(),
                    skippable: step.skippable,
                };
                SequenceItem::Group { tool_group }
            } else {
                return Err(McpError::invalid_params(
                    "Each step must have either tool_name (for single tools) or group_name (for groups)",
                    Some(json!({"invalid_step": step})),
                ));
            };
            sequence_items.push(item);
        }

        // Add troubleshooting steps to the sequence (they won't execute unless jumped to via fallback_id)
        if let Some(troubleshooting) = &args.troubleshooting {
            info!(
                "Adding {} troubleshooting steps to workflow (accessible only via fallback_id)",
                troubleshooting.len()
            );
            for step in troubleshooting {
                let item = if let Some(tool_name) = &step.tool_name {
                    // Parse delay from either delay_ms or human-readable delay field
                    let delay_ms = if let Some(delay_str) = &step.delay {
                        match crate::duration_parser::parse_duration(delay_str) {
                            Ok(ms) => Some(ms),
                            Err(e) => {
                                warn!("Failed to parse delay '{}': {}", delay_str, e);
                                step.delay_ms // Fall back to delay_ms
                            }
                        }
                    } else {
                        step.delay_ms
                    };

                    let tool_call = ToolCall {
                        tool_name: tool_name.clone(),
                        arguments: step.arguments.clone().unwrap_or(serde_json::json!({})),
                        continue_on_error: step.continue_on_error,
                        delay_ms,
                        id: step.id.clone(),
                    };
                    SequenceItem::Tool { tool_call }
                } else if let Some(group_name) = &step.group_name {
                    let tool_group = ToolGroup {
                        group_name: group_name.clone(),
                        steps: step
                            .steps
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|s| ToolCall {
                                tool_name: s.tool_name,
                                arguments: s.arguments,
                                continue_on_error: s.continue_on_error,
                                delay_ms: s.delay_ms,
                                id: s.id,
                            })
                            .collect(),
                        skippable: step.skippable,
                    };
                    SequenceItem::Group { tool_group }
                } else {
                    return Err(McpError::invalid_params(
                        "Each troubleshooting step must have either tool_name (for single tools) or group_name (for groups)",
                        Some(json!({"invalid_step": step})),
                    ));
                };
                sequence_items.push(item);
            }
        }

        // ---------------------------
        // PRE-FLIGHT CHECK: Chrome Extension Health
        // ---------------------------
        // Check if workflow contains any execute_browser_script steps
        // If yes, verify Chrome extension is connected before starting execution
        let has_browser_script_steps = steps.iter().any(|step| {
            step.tool_name
                .as_ref()
                .map(|t| t == "execute_browser_script")
                .unwrap_or(false)
        }) || args
            .troubleshooting
            .as_ref()
            .map(|t| {
                t.iter().any(|step| {
                    step.tool_name
                        .as_ref()
                        .map(|t| t == "execute_browser_script")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if has_browser_script_steps {
            info!(
                "Workflow contains execute_browser_script steps - checking Chrome extension health"
            );

            // Initialize the extension bridge (starts WebSocket server on port 17373)
            let bridge = terminator::extension_bridge::ExtensionBridge::global().await;

            // Trigger browser activity to wake up the extension
            // Navigate to a blank page in CHROME to trigger the extension's content script
            info!("Triggering Chrome browser activity to wake up extension...");
            match terminator::Desktop::new_default() {
                Ok(desktop) => {
                    match desktop.open_url("about:blank", Some(terminator::Browser::Chrome)) {
                        Ok(_chrome_window) => {
                            info!("Chrome navigation triggered successfully");
                            // Give the extension a moment to detect the page load and connect
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        Err(e) => {
                            warn!("Failed to navigate Chrome: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to create Desktop instance: {:?}", e);
                }
            }

            // Now test extension connection with a minimal eval (ping)
            // This uses the same 10-second retry logic as execute_browser_script
            // The ping executes in the about:blank tab we just opened
            info!("Testing Chrome extension connection with ping script...");
            let ping_result = bridge
                .eval_in_active_tab("true", Duration::from_secs(10))
                .await;

            let is_connected = ping_result.is_ok() && ping_result.as_ref().unwrap().is_some();

            // Get updated health status after connection attempt
            let bridge_health =
                terminator::extension_bridge::ExtensionBridge::health_status().await;
            let status = bridge_health
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let clients = bridge_health
                .get("clients")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if !is_connected {
                warn!(
                    "Chrome extension not connected before workflow execution: status={}, clients={}",
                    status,
                    clients
                );

                // End workflow span
                workflow_span.set_status(false, "Chrome extension not available");
                workflow_span.end();

                return Err(McpError::invalid_params(
                    "Chrome extension bridge is not connected",
                    Some(json!({
                        "error_type": "extension_unavailable",
                        "extension_status": status,
                        "extension_clients": clients,
                        "workflow_file": args.url.as_ref(),
                        "browser_script_steps_count": sequence_items.iter().filter(|item| {
                            matches!(item, SequenceItem::Tool { tool_call } if tool_call.tool_name == "execute_browser_script")
                        }).count(),
                        "troubleshooting": [
                            "Verify Chrome extension is installed and enabled at chrome://extensions",
                            "Extension ID: Check terminator browser-extension folder",
                            "Ensure Chrome browser is running",
                            "Check if WebSocket port 17373 is accessible",
                            "Try restarting Chrome browser",
                            "Review extension bridge health: http://127.0.0.1:3000/health (HTTP transport)"
                        ],
                        "health_details": bridge_health
                    })),
                ));
            }

            info!(
                "✅ Chrome extension healthy: {} client(s) connected",
                clients
            );

            // Close the about:blank tab now that we've confirmed the extension works
            match terminator::Desktop::new_default() {
                Ok(desktop) => {
                    match desktop.press_key("{Ctrl}w").await {
                        Ok(_) => {
                            info!("Closed about:blank tab with Ctrl+W");
                            // Wait for tab close to complete before starting workflow
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        Err(e) => warn!("Failed to close about:blank tab: {:?}", e),
                    }
                }
                Err(e) => warn!("Failed to create Desktop instance for tab cleanup: {:?}", e),
            }
        }

        // ---------------------------
        // Fallback-enabled execution loop (while-based)
        // ---------------------------

        let mut results = Vec::new();
        let mut sequence_had_errors = false;
        let mut critical_error_occurred = false;
        let mut used_fallback = false; // Track if any fallback was used
        let start_time = chrono::Utc::now();

        let mut current_index: usize = start_from_index;
        let max_iterations = sequence_items.len() * 10; // Prevent infinite fallback loops
        let mut iterations = 0usize;

        // Track whether we've jumped to troubleshooting
        let mut jumped_to_troubleshooting = false;

        // Detect if we're starting directly in the troubleshooting section
        if start_from_index >= main_steps_len {
            jumped_to_troubleshooting = true;
            info!(
                "Starting execution directly in troubleshooting section at step index {} (troubleshooting step #{})",
                start_from_index,
                start_from_index - main_steps_len + 1
            );
        }

        // Get follow_fallback setting
        // - Default to true for unbounded execution (no end_at_step) to allow troubleshooting fallbacks
        // - Default to false for bounded execution (with end_at_step) to respect boundaries
        let follow_fallback = args.follow_fallback.unwrap_or(args.end_at_step.is_none());
        if args.end_at_step.is_some() {
            info!("follow_fallback={} for bounded execution", follow_fallback);
        } else {
            info!("follow_fallback={} for unbounded execution (defaulting to true for troubleshooting access)", follow_fallback);
        }

        // Log if we're skipping steps
        if start_from_index > 0 {
            let step_type = if start_from_index >= main_steps_len {
                "troubleshooting"
            } else {
                "main workflow"
            };
            info!(
                "Starting from {} step at index {}",
                step_type, start_from_index
            );
        }

        // Log if we're stopping at a specific step
        if end_at_index < sequence_items.len() - 1 {
            let step_type = if end_at_index >= main_steps_len {
                "troubleshooting"
            } else {
                "main workflow"
            };
            info!(
                "Will stop after {} step at index {} (inclusive)",
                step_type, end_at_index
            );
        }

        while current_index < sequence_items.len()
            && (current_index <= end_at_index || (follow_fallback && jumped_to_troubleshooting))
            && iterations < max_iterations
        {
            iterations += 1;

            // Check if the request has been cancelled
            if request_context.ct.is_cancelled() {
                warn!("Request cancelled by user, stopping sequence execution");
                return Err(McpError::internal_error(
                    "Request cancelled by user",
                    Some(json!({"code": -32001, "reason": "user_cancelled"})),
                ));
            }

            // Get the original step from either main steps or troubleshooting steps
            let original_step = if current_index < main_steps_len {
                args.steps.as_ref().and_then(|s| s.get(current_index))
            } else {
                args.troubleshooting
                    .as_ref()
                    .and_then(|t| t.get(current_index - main_steps_len))
            };
            if let Some(step) = original_step {
                if let Some(tool_name) = &step.tool_name {
                    info!(
                        "Step {} BEGIN tool='{}' id='{}' retries={} if_expr={:?} fallback_id={:?} jumps={}",
                        current_index,
                        tool_name,
                        step.id.as_deref().unwrap_or(""),
                        step.retries.unwrap_or(0),
                        step.r#if,
                        step.fallback_id,
                        step.jumps.as_ref().map(|j| j.len()).unwrap_or(0)
                    );
                } else if let Some(group_name) = &step.group_name {
                    info!(
                        "Step {} BEGIN group='{}' id='{}' steps={}",
                        current_index,
                        group_name,
                        step.id.as_deref().unwrap_or(""),
                        step.steps.as_ref().map(|v| v.len()).unwrap_or(0)
                    );
                }
            }

            // Extract values from the step if it exists
            let (if_expr, retries, fallback_id_opt) = if let Some(step) = original_step {
                (
                    step.r#if.clone(),
                    step.retries.unwrap_or(0),
                    step.fallback_id.clone(),
                )
            } else {
                (None, 0, None)
            };

            let is_always_step = if_expr.as_deref().is_some_and(|s| s.trim() == "always()");

            // If a critical error occurred and this step is NOT an 'always' step, skip it.
            if critical_error_occurred && !is_always_step {
                results.push(json!({
                    "index": current_index,
                    "status": "skipped",
                    "reason": "Skipped due to a previous unrecoverable error in the sequence."
                }));
                current_index += 1;
                continue;
            }

            // 1. Evaluate condition, unless it's an 'always' step.
            if let Some(cond_str) = &if_expr {
                let execution_context =
                    Self::create_flattened_execution_context(&execution_context_map);
                if !is_always_step
                    && !crate::expression_eval::evaluate(cond_str, &execution_context)
                {
                    info!(
                        "Skipping step {} due to if expression not met: `{}`",
                        current_index, cond_str
                    );
                    results.push(json!({
                        "index": current_index,
                        "status": "skipped",
                        "reason": format!("if_expr not met: {}", cond_str)
                    }));
                    current_index += 1;
                    continue;
                }
            }

            // 2. Execute with retries
            let mut final_result = json!(null);
            let mut step_error_occurred = false;
            let total_steps = sequence_items.len();

            for attempt in 0..=retries {
                let item = &mut sequence_items[current_index];
                match item {
                    SequenceItem::Tool { tool_call } => {
                        // Special internal pseudo-tool to set env for subsequent steps
                        let tool_name_normalized = tool_call
                            .tool_name
                            .strip_prefix("mcp_terminator-mcp-agent_")
                            .unwrap_or(&tool_call.tool_name)
                            .to_string();

                        // Substitute variables in arguments before execution
                        let execution_context =
                            Self::create_flattened_execution_context(&execution_context_map);
                        let mut substituted_args = tool_call.arguments.clone();
                        substitute_variables(&mut substituted_args, &execution_context);

                        // Inject workflow variables and accumulated env for run_command and execute_browser_script
                        if matches!(
                            tool_call.tool_name.as_str(),
                            "run_command" | "execute_browser_script"
                        ) {
                            // Get env object or create empty one
                            let mut env_obj = substituted_args
                                .get("env")
                                .and_then(|v| v.as_object())
                                .cloned()
                                .unwrap_or_else(serde_json::Map::new);

                            // Always inject workflow variables (scripts depend on them)
                            // Extract default values from VariableDefinition objects for consistency
                            if let Some(workflow_vars) = &args.variables {
                                let mut resolved_vars = serde_json::Map::new();

                                // Step 1: Start with defaults from variable schema
                                for (key, def) in workflow_vars {
                                    if let Some(default_value) = &def.default {
                                        resolved_vars.insert(key.clone(), default_value.clone());
                                    }
                                }

                                // Step 2: Deep merge runtime inputs (overrides defaults)
                                // This allows UI-provided parameters to override variable defaults
                                if let Some(inputs) = &args.inputs {
                                    tracing::debug!(
                                        "[workflow_variables] Before merge: {}",
                                        serde_json::to_string(&resolved_vars).unwrap_or_default()
                                    );
                                    Self::deep_merge_json(&mut resolved_vars, inputs);
                                    tracing::debug!(
                                        "[workflow_variables] After merge: {}",
                                        serde_json::to_string(&resolved_vars).unwrap_or_default()
                                    );
                                }

                                env_obj.insert(
                                    "_workflow_variables".to_string(),
                                    json!(resolved_vars),
                                );
                            }

                            // Only inject accumulated env if explicitly in verbose/debug mode
                            if args.include_detailed_results.unwrap_or(false) {
                                // Add accumulated env from execution context as special key
                                if let Some(accumulated_env) = execution_context.get("env") {
                                    env_obj.insert(
                                        "_accumulated_env".to_string(),
                                        accumulated_env.clone(),
                                    );
                                }
                            }

                            // Update the arguments
                            if let Some(args_obj) = substituted_args.as_object_mut() {
                                args_obj.insert("env".to_string(), json!(env_obj));
                            }
                        }

                        // Start step telemetry span
                        let step_id = original_step.and_then(|s| s.id.as_deref());
                        let mut step_span = StepSpan::new(&tool_call.tool_name, step_id);
                        step_span.set_attribute("step.number", (current_index + 1).to_string());
                        step_span.set_attribute("step.total", total_steps.to_string());
                        if attempt > 0 {
                            step_span.set_attribute("step.retry_attempt", attempt.to_string());
                        }

                        // Add event for step started
                        workflow_span.add_event(
                            "step.started",
                            vec![
                                ("step.tool", tool_call.tool_name.clone()),
                                ("step.index", current_index.to_string()),
                            ],
                        );

                        let (result, error_occurred) = self
                            .execute_single_tool(
                                peer.clone(),
                                request_context.clone(),
                                &tool_call.tool_name,
                                &substituted_args,
                                tool_call.continue_on_error.unwrap_or(false),
                                current_index,
                                include_detailed,
                                original_step.and_then(|s| s.id.as_deref()),
                            )
                            .await;

                        final_result = result.clone();

                        // NEW: Store tool result in env if step has an ID (for ALL tools, not just scripts)
                        if let Some(step_id) = original_step.and_then(|s| s.id.as_deref()) {
                            let result_key = format!("{step_id}_result");
                            let status_key = format!("{step_id}_status");

                            // Extract the meaningful content from the result
                            let mut result_content =
                                if let Some(result_obj) = final_result.get("result") {
                                    // For tools, extract the actual content
                                    if let Some(content) = result_obj.get("content") {
                                        content.clone()
                                    } else {
                                        result_obj.clone()
                                    }
                                } else {
                                    // Fallback to the entire result if no nested structure
                                    final_result.clone()
                                };

                            // REMOVE server_logs before storing in env (they're debug data, not operational data)
                            if let Some(obj) = result_content.as_object_mut() {
                                if obj.contains_key("server_logs") {
                                    let log_count = obj
                                        .get("server_logs")
                                        .and_then(|logs| logs.as_array())
                                        .map(|arr| arr.len())
                                        .unwrap_or(0);
                                    obj.remove("server_logs");
                                    debug!(
                                        "Removed {} server_logs from {}_result before storing in env",
                                        log_count, step_id
                                    );
                                }
                            }

                            // Store at root level for easier expression access
                            execution_context_map
                                .insert(result_key.clone(), result_content.clone());
                            execution_context_map
                                .insert(status_key.clone(), final_result["status"].clone());

                            // Also store in env
                            if let Some(env_value) = execution_context_map.get_mut("env") {
                                if let Some(env_map) = env_value.as_object_mut() {
                                    env_map.insert(result_key.clone(), result_content);
                                    env_map
                                        .insert(status_key.clone(), final_result["status"].clone());

                                    info!(
                                        "Stored tool result for step '{}' as '{}' at root and env levels",
                                        step_id, result_key
                                    );

                                    // Save state after storing tool result
                                    if let Some(url) = &args.url {
                                        Self::save_workflow_state(
                                            url,
                                            Some(step_id),
                                            current_index,
                                            env_value,
                                        )
                                        .await
                                        .ok(); // Don't fail the workflow if state save fails
                                    }
                                }
                            }
                        }

                        // Update step span status and end it
                        // Support both 'status' field and 'success' field
                        let success = result["status"] == "success"
                            || result["success"] == true
                            || (result["status"].is_null() && result["success"] != false);
                        step_span.set_status(
                            success,
                            if !success {
                                result["error"].as_str()
                            } else {
                                None
                            },
                        );
                        step_span.end();

                        // Add workflow event for step completion
                        workflow_span.add_event(
                            "step.completed",
                            vec![
                                ("step.tool", tool_call.tool_name.clone()),
                                ("step.index", current_index.to_string()),
                                (
                                    "step.status",
                                    result["status"].as_str().unwrap_or("unknown").to_string(),
                                ),
                            ],
                        );

                        // Define reserved keys that shouldn't auto-merge
                        const RESERVED_KEYS: &[&str] =
                            &["status", "error", "logs", "duration_ms", "set_env"];

                        // Merge env updates from engine/script-based steps into the internal context
                        if (tool_name_normalized == "execute_browser_script"
                            || tool_name_normalized == "run_command")
                            && final_result["status"] == "success"
                        {
                            // Helper to merge updates into the env context map
                            let mut merge_env_obj = |update_val: &serde_json::Value| {
                                if let Some(update_map) = update_val.as_object() {
                                    if let Some(env_value) = execution_context_map.get_mut("env") {
                                        if let Some(env_map) = env_value.as_object_mut() {
                                            for (k, v) in update_map.iter() {
                                                env_map.insert(k.clone(), v.clone());
                                            }
                                        }
                                    }
                                }
                            };

                            // Special handling for execute_browser_script
                            if tool_name_normalized == "execute_browser_script" {
                                // Browser scripts return their result as a plain string in final_result["result"]["content"][0]["result"]
                                if let Some(result_str) = final_result
                                    .get("result")
                                    .and_then(|r| r.get("content"))
                                    .and_then(|c| c.as_array())
                                    .and_then(|arr| arr.first())
                                    .and_then(|item| item.get("result"))
                                    .and_then(|r| r.as_str())
                                {
                                    info!(
                                        "[execute_browser_script] Browser script returned: {}",
                                        result_str
                                    );
                                    // Try to parse the browser script result as JSON
                                    match serde_json::from_str::<serde_json::Value>(result_str) {
                                        Ok(parsed_json) => {
                                            info!("[execute_browser_script] Successfully parsed browser result as JSON");

                                            // First handle explicit set_env for backward compatibility
                                            if let Some(set_env) = parsed_json.get("set_env") {
                                                info!("[execute_browser_script] Found set_env in browser script result, merging into context");
                                                merge_env_obj(set_env);
                                            }

                                            // Then auto-merge non-reserved fields
                                            if let Some(obj) = parsed_json.as_object() {
                                                if let Some(env_value) =
                                                    execution_context_map.get_mut("env")
                                                {
                                                    if let Some(env_map) = env_value.as_object_mut()
                                                    {
                                                        for (k, v) in obj {
                                                            if RESERVED_KEYS.contains(&k.as_str()) {
                                                                warn!(
                                                                    "[execute_browser_script] Script returned reserved field '{}' which will be ignored. Reserved fields: {:?}",
                                                                    k, RESERVED_KEYS
                                                                );
                                                            } else {
                                                                env_map
                                                                    .insert(k.clone(), v.clone());
                                                                info!("[execute_browser_script] Auto-merged field '{}' to env", k);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            info!("[execute_browser_script] Browser result is not JSON: {}", e);
                                        }
                                    }
                                } else {
                                    info!("[execute_browser_script] Could not extract browser script result string from response structure");
                                }
                            } else if tool_name_normalized == "run_command" {
                                // Original logic for run_command
                                if let Some(content_arr) = final_result
                                    .get("result")
                                    .and_then(|r| r.get("content"))
                                    .and_then(|c| c.as_array())
                                {
                                    for item in content_arr {
                                        // Typical engine payload is under item.result
                                        if let Some(res) = item.get("result") {
                                            // First handle explicit set_env/env for backward compatibility
                                            if let Some(v) =
                                                res.get("set_env").or_else(|| res.get("env"))
                                            {
                                                merge_env_obj(v);
                                            }
                                        }
                                        // Also support top-level set_env/env directly on the item
                                        if let Some(v) =
                                            item.get("set_env").or_else(|| item.get("env"))
                                        {
                                            merge_env_obj(v);
                                        }
                                    }

                                    // Auto-merge non-reserved fields from run_command results
                                    for item in content_arr {
                                        if let Some(res) = item.get("result") {
                                            if let Some(obj) = res.as_object() {
                                                if let Some(env_value) =
                                                    execution_context_map.get_mut("env")
                                                {
                                                    if let Some(env_map) = env_value.as_object_mut()
                                                    {
                                                        for (k, v) in obj {
                                                            if !RESERVED_KEYS.contains(&k.as_str())
                                                            {
                                                                env_map
                                                                    .insert(k.clone(), v.clone());
                                                                info!("[run_command] Auto-merged field '{}' to env", k);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // NEW: Auto-merge non-reserved fields from root level
                                    // This enables scripts to return data directly without wrapping in 'result'
                                    for item in content_arr {
                                        // Try two approaches:
                                        // 1. If the result is a JSON string, parse it and merge fields
                                        // 2. If the item itself is an object with fields, merge those

                                        // Approach 1: Parse JSON string from result field
                                        if let Some(result_str) =
                                            item.get("result").and_then(|r| r.as_str())
                                        {
                                            // Try to parse the result string as JSON
                                            if let Ok(parsed_json) =
                                                serde_json::from_str::<serde_json::Value>(
                                                    result_str,
                                                )
                                            {
                                                if let Some(parsed_obj) = parsed_json.as_object() {
                                                    if let Some(env_value) =
                                                        execution_context_map.get_mut("env")
                                                    {
                                                        if let Some(env_map) =
                                                            env_value.as_object_mut()
                                                        {
                                                            // Define structural keys that should not be merged
                                                            const STRUCTURAL_KEYS: &[&str] = &[
                                                                "result", "action", "mode",
                                                                "engine", "content",
                                                            ];

                                                            for (k, v) in parsed_obj {
                                                                // Check if it's a reserved key
                                                                if RESERVED_KEYS
                                                                    .contains(&k.as_str())
                                                                {
                                                                    warn!(
                                                                        "[run_command] Script returned reserved field '{}' at root level which will be ignored. Reserved fields: {:?}",
                                                                        k, RESERVED_KEYS
                                                                    );
                                                                    continue;
                                                                }

                                                                // Skip structural keys silently
                                                                if STRUCTURAL_KEYS
                                                                    .contains(&k.as_str())
                                                                {
                                                                    continue;
                                                                }

                                                                // Merge the field (overwrite to ensure updates)
                                                                env_map
                                                                    .insert(k.clone(), v.clone());
                                                                info!("[run_command] Auto-merged root field '{}' from parsed JSON to env", k);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // Approach 2: Direct object fields (for backward compatibility)
                                        if let Some(obj) = item.as_object() {
                                            if let Some(env_value) =
                                                execution_context_map.get_mut("env")
                                            {
                                                if let Some(env_map) = env_value.as_object_mut() {
                                                    // Define structural keys that should not be merged
                                                    const STRUCTURAL_KEYS: &[&str] = &[
                                                        "result", "action", "mode", "engine",
                                                        "content",
                                                    ];

                                                    for (k, v) in obj {
                                                        // Check if it's a reserved key
                                                        if RESERVED_KEYS.contains(&k.as_str()) {
                                                            warn!(
                                                                "[run_command] Script returned reserved field '{}' at root level which will be ignored. Reserved fields: {:?}",
                                                                k, RESERVED_KEYS
                                                            );
                                                            continue;
                                                        }

                                                        // Skip structural keys silently
                                                        if STRUCTURAL_KEYS.contains(&k.as_str()) {
                                                            continue;
                                                        }

                                                        // Merge the field (overwrite to ensure updates)
                                                        env_map.insert(k.clone(), v.clone());
                                                        debug!("[run_command] Auto-merged root field '{}' to env", k);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // NEW: Save state after env update
                            if let Some(url) = &args.url {
                                if let Some(env_value) = execution_context_map.get("env") {
                                    Self::save_workflow_state(
                                        url,
                                        original_step.and_then(|s| s.id.as_deref()),
                                        current_index,
                                        env_value,
                                    )
                                    .await
                                    .ok(); // Don't fail the workflow if state save fails
                                }
                            }
                        }
                        // Check for success using both 'status' and 'success' fields
                        if result["status"] == "success"
                            || result["success"] == true
                            || (result["status"].is_null() && result["success"] != false)
                        {
                            // Apply delay after successful execution
                            if let Some(delay_ms) = tool_call.delay_ms {
                                if delay_ms > 0 {
                                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                                }
                            }
                            break;
                        }

                        if error_occurred {
                            // Only mark as critical if there's no fallback to handle it
                            if fallback_id_opt.is_none() {
                                critical_error_occurred = true;
                                if let Some(id) = original_step.and_then(|s| s.id.as_deref()) {
                                    tracing::warn!(
                                        step_id = %id,
                                        tool = %tool_call.tool_name,
                                        attempt = attempt + 1,
                                        skippable = %tool_call.continue_on_error.unwrap_or(false),
                                        has_fallback = false,
                                        "Tool failed with unrecoverable error (no fallback)"
                                    );
                                } else {
                                    tracing::warn!(
                                        tool = %tool_call.tool_name,
                                        attempt = attempt + 1,
                                        skippable = %tool_call.continue_on_error.unwrap_or(false),
                                        has_fallback = false,
                                        "Tool failed with unrecoverable error (no fallback)"
                                    );
                                }
                            } else {
                                // Has fallback, log but don't mark as critical
                                if let Some(id) = original_step.and_then(|s| s.id.as_deref()) {
                                    tracing::info!(
                                        step_id = %id,
                                        tool = %tool_call.tool_name,
                                        fallback_id = %fallback_id_opt.as_ref().unwrap(),
                                        "Tool failed but has fallback configured"
                                    );
                                } else {
                                    tracing::info!(
                                        tool = %tool_call.tool_name,
                                        fallback_id = %fallback_id_opt.as_ref().unwrap(),
                                        "Tool failed but has fallback configured"
                                    );
                                }
                            }
                        }
                        step_error_occurred = true;
                        sequence_had_errors = true;

                        if let Some(delay_ms) = tool_call.delay_ms {
                            if delay_ms > 0 {
                                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            }
                        }
                    }
                    SequenceItem::Group { tool_group } => {
                        let mut group_had_errors = false;
                        let mut group_results = Vec::new();
                        let is_skippable = tool_group.skippable.unwrap_or(false);

                        for (step_index, step_tool_call) in tool_group.steps.iter_mut().enumerate()
                        {
                            // Substitute variables in arguments before execution
                            let execution_context =
                                Self::create_flattened_execution_context(&execution_context_map);
                            let mut substituted_args = step_tool_call.arguments.clone();
                            substitute_variables(&mut substituted_args, &execution_context);

                            let (result, error_occurred) = self
                                .execute_single_tool(
                                    peer.clone(),
                                    request_context.clone(),
                                    &step_tool_call.tool_name,
                                    &substituted_args,
                                    step_tool_call.continue_on_error.unwrap_or(false),
                                    step_index,
                                    include_detailed,
                                    step_tool_call.id.as_deref(), // Use step ID if available
                                )
                                .await;

                            group_results.push(result.clone());

                            if let Some(delay_ms) = step_tool_call.delay_ms {
                                if delay_ms > 0 {
                                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                                }
                            }

                            // Check for failure using both 'status' and 'success' fields
                            let tool_failed = !(result["status"] == "success"
                                || result["success"] == true
                                || (result["status"].is_null() && result["success"] != false));
                            if tool_failed {
                                group_had_errors = true;
                                if error_occurred || is_skippable {
                                    if error_occurred && !is_skippable {
                                        // Only mark as critical if there's no fallback to handle it
                                        if fallback_id_opt.is_none() {
                                            critical_error_occurred = true;
                                        }
                                    }
                                    tracing::warn!(
                                        group = %tool_group.group_name,
                                        tool = %step_tool_call.tool_name,
                                        step_index = step_index,
                                        step_id = %step_tool_call.id.clone().unwrap_or_default(),
                                        skippable = %is_skippable,
                                        has_fallback = fallback_id_opt.is_some(),
                                        "Group step failed; breaking out of group"
                                    );
                                    break;
                                }
                            }
                        }

                        let group_status = if group_had_errors {
                            "partial_success"
                        } else {
                            "success"
                        };

                        if group_status != "success" {
                            sequence_had_errors = true;
                            step_error_occurred = true;
                        }

                        if group_had_errors && !is_skippable && stop_on_error {
                            // Only mark as critical if there's no fallback to handle it
                            if fallback_id_opt.is_none() {
                                critical_error_occurred = true;
                            }
                        }

                        final_result = json!({
                            "group_name": &tool_group.group_name,
                            "status": group_status,
                            "results": group_results
                        });

                        if !group_had_errors {
                            break; // Group succeeded, break retry loop.
                        }
                    }
                }
                if attempt < retries {
                    warn!(
                        "Step {} failed on attempt {}/{}. Retrying...",
                        current_index,
                        attempt + 1,
                        retries
                    );
                    tokio::time::sleep(Duration::from_millis(500)).await; // Wait before retry
                }
            }

            results.push(final_result);

            // Decide next index based on success or fallback
            let step_succeeded = !step_error_occurred;
            let step_status_str = if step_succeeded { "success" } else { "failed" };
            if let Some(tool_name) = original_step.and_then(|s| s.tool_name.as_ref()) {
                info!(
                    "Step {} END tool='{}' id='{}' status={}",
                    current_index,
                    tool_name,
                    original_step.and_then(|s| s.id.as_deref()).unwrap_or(""),
                    step_status_str
                );
            } else if let Some(group_name) = original_step.and_then(|s| s.group_name.as_ref()) {
                info!(
                    "Step {} END group='{}' id='{}' status={}",
                    current_index,
                    group_name,
                    original_step.and_then(|s| s.id.as_deref()).unwrap_or(""),
                    step_status_str
                );
            }

            if step_succeeded {
                // Check for conditional jumps on success
                let mut performed_jump = false;

                // Check if we should skip jump evaluation at the end_at_step boundary
                // When end_at_step is specified, jumps are skipped by default at the boundary
                // to provide predictable execution bounds. Users can override this with
                // --execute-jumps-at-end to allow jumps even at the boundary (e.g., for loops).
                let execute_jumps_at_end = args.execute_jumps_at_end.unwrap_or(false);
                let skip_jumps = current_index == end_at_index && !execute_jumps_at_end;

                if skip_jumps {
                    info!(
                        "Skipping jump evaluation at end_at_step boundary (step index {}). Use --execute-jumps-at-end to enable jumps at boundary.",
                        current_index
                    );
                } else if let Some(jumps) = original_step.and_then(|s| s.jumps.as_ref()) {
                    if !jumps.is_empty() {
                        info!(
                            "Evaluating {} jump condition(s) for step {}",
                            jumps.len(),
                            current_index
                        );

                        let execution_context =
                            Self::create_flattened_execution_context(&execution_context_map);

                        for (idx, jump) in jumps.iter().enumerate() {
                            debug!(
                                "Evaluating jump condition {}/{}: {}",
                                idx + 1,
                                jumps.len(),
                                jump.condition
                            );

                            if crate::expression_eval::evaluate(&jump.condition, &execution_context)
                            {
                                // This condition matched - perform the jump
                                if let Some(&target_idx) = id_to_index.get(&jump.to_id) {
                                    let reason = jump
                                        .reason
                                        .as_ref()
                                        .map(|r| format!(": \"{r}\""))
                                        .unwrap_or_default();

                                    info!(
                                        "Step {} succeeded. Jump condition {}/{} matched{}. Jumping to '{}' (index {})",
                                        current_index, idx + 1, jumps.len(), reason, jump.to_id, target_idx
                                    );

                                    // Check if jumping into troubleshooting section
                                    if target_idx >= main_steps_len && !jumped_to_troubleshooting {
                                        jumped_to_troubleshooting = true;
                                        info!(
                                            "Entered troubleshooting section via conditional jump"
                                        );
                                    }

                                    current_index = target_idx;
                                    performed_jump = true;
                                    break; // Stop evaluating remaining conditions
                                } else {
                                    warn!(
                                        "Jump target '{}' not found for step {} condition {}. Continuing to next condition.",
                                        jump.to_id, current_index, idx + 1
                                    );
                                }
                            } else {
                                debug!("Jump condition {}/{} did not match", idx + 1, jumps.len());
                            }
                        }

                        if !performed_jump {
                            debug!("No jump conditions matched for step {}", current_index);
                        }
                    }
                }

                // Only increment if we didn't jump
                if !performed_jump {
                    // For successful steps, check if we're about to enter troubleshooting section
                    if !jumped_to_troubleshooting && current_index >= main_steps_len - 1 {
                        // We're at or past the last main step and haven't jumped to troubleshooting
                        // Exit the loop to prevent entering troubleshooting during normal flow
                        info!("Completed all main workflow steps successfully");
                        break;
                    }
                    current_index += 1;
                }
            } else if let Some(fb_id) = fallback_id_opt {
                if let Some(&fb_idx) = id_to_index.get(&fb_id) {
                    // Check if we should follow this fallback based on end_at_step and follow_fallback setting
                    let should_follow_fallback = if args.end_at_step.is_some()
                        && current_index >= end_at_index
                    {
                        // We're at or past end_at_step boundary
                        if follow_fallback {
                            info!(
                                "Step {} failed at end_at_step boundary. Following fallback to '{}' (follow_fallback=true).",
                                current_index, fb_id
                            );
                            true
                        } else {
                            info!(
                                "Step {} failed at end_at_step boundary. NOT following fallback '{}' (follow_fallback=false).",
                                current_index, fb_id
                            );
                            false
                        }
                    } else {
                        // Normal execution, always follow fallback
                        true
                    };

                    if should_follow_fallback {
                        info!(
                            "Step {} failed. Jumping to fallback step with id '{}' (index {}).",
                            current_index, fb_id, fb_idx
                        );

                        // Mark that we used a fallback
                        used_fallback = true;

                        // Check if we're jumping into the troubleshooting section
                        if fb_idx >= main_steps_len {
                            jumped_to_troubleshooting = true;
                            info!("Entered troubleshooting section via fallback");
                        }

                        current_index = fb_idx;
                    } else {
                        // Don't follow fallback, treat as normal failure
                        // Break the loop since we're at end_at_step and not following fallback
                        info!(
                            "Stopping execution at end_at_step boundary without following fallback"
                        );
                        break;
                    }
                } else {
                    warn!(
                        "fallback_id '{}' for step {} not found. Continuing to next step.",
                        fb_id, current_index
                    );
                    current_index += 1;
                }
            } else {
                // Step failed with no fallback
                current_index += 1;
            }
        }

        if iterations >= max_iterations {
            warn!("Maximum iteration count reached. Possible infinite fallback loop detected.");
        }

        let total_duration = (chrono::Utc::now() - start_time).num_milliseconds();

        // Determine final status - simple success or failure
        let final_status = if !sequence_had_errors {
            "success"
        } else {
            "failed"
        };
        info!(
            "execute_sequence completed: status={}, executed_tools={}, total_duration_ms={}",
            final_status,
            results.len(),
            total_duration
        );

        let mut summary = json!({
            "action": "execute_sequence",
            "status": final_status,
            "total_tools": sequence_items.len(),
            "executed_tools": results.len(),
            "total_duration_ms": total_duration,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "used_fallback": used_fallback,
            "results": results,
            "env": execution_context_map.get("env").cloned().unwrap_or_else(|| json!({})),
        });

        // Support both 'output_parser' (legacy) and 'output' (simplified)
        let parser_def = args.output_parser.as_ref().or(args.output.as_ref());

        // Skip output parser when end_at_step is specified (partial execution)
        if let Some(parser_def) = parser_def {
            if args.end_at_step.is_some() {
                warn!(
                    "Skipping output parser for partial workflow execution (end_at_step specified)"
                );
                if let Some(obj) = summary.as_object_mut() {
                    obj.insert(
                        "parser_skipped".to_string(),
                        json!("Partial execution with end_at_step"),
                    );
                }
            } else {
                // Apply variable substitution to the output_parser field
                let mut parser_json = parser_def.clone();
                let execution_context =
                    Self::create_flattened_execution_context(&execution_context_map);
                substitute_variables(&mut parser_json, &execution_context);

                match output_parser::run_output_parser(&parser_json, &summary).await {
                    Ok(Some(parsed_data)) => {
                        // Check if the parsed data is wrapped in a 'result' field and unwrap it
                        // This handles the case where JavaScript execution via scripting_engine returns
                        // {result: <actual_parser_output>, logs: [...]} wrapper structure.
                        // We need to extract the actual parser output from the wrapper to ensure
                        // the CLI and downstream consumers receive the parser's intended structure.
                        let final_data = if let Some(result) = parsed_data.get("result") {
                            // Log that we're unwrapping for debugging visibility
                            info!(
                            "[output_parser] Unwrapping parser result from JavaScript execution wrapper"
                        );
                            // Unwrap the result field to get the actual parser output
                            result.clone()
                        } else {
                            // Use as-is if not wrapped (backward compatibility with direct returns)
                            parsed_data
                        };

                        if let Some(obj) = summary.as_object_mut() {
                            obj.insert("parsed_output".to_string(), final_data);
                        }
                    }
                    Ok(None) => {
                        if let Some(obj) = summary.as_object_mut() {
                            obj.insert("parsed_output".to_string(), json!({}));
                        }
                    }
                    Err(e) => {
                        if let Some(obj) = summary.as_object_mut() {
                            obj.insert("parser_error".to_string(), json!(e.to_string()));
                        }
                    }
                }
            }
        }
        if final_status != "success" {
            // Capture minimal structured debug info so failures are not opaque
            let debug_info = json!({
                "final_status": final_status,
                "had_critical_error": critical_error_occurred,
                "had_errors": sequence_had_errors,
                "used_fallback": used_fallback,
                "executed_count": results.len(),
            });

            if let Some(obj) = summary.as_object_mut() {
                obj.insert("debug_info_on_failure".to_string(), debug_info);
            }
        }

        let contents = vec![Content::json(summary.clone())?];

        // End workflow span with appropriate status
        let span_success = matches!(final_status, "success");
        let span_message = if span_success {
            "Workflow completed successfully"
        } else {
            "Workflow failed"
        };

        workflow_span.set_status(span_success, span_message);
        workflow_span.add_event(
            "workflow.completed",
            vec![
                ("workflow.total_steps", results.len().to_string()),
                ("workflow.final_status", final_status.to_string()),
                ("workflow.used_fallback", used_fallback.to_string()),
            ],
        );
        workflow_span.end();

        Ok(CallToolResult::success(contents))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute_single_tool(
        &self,
        peer: Peer<RoleServer>,
        request_context: RequestContext<RoleServer>,
        tool_name: &str,
        arguments: &Value,
        is_skippable: bool,
        index: usize,
        include_detailed: bool,
        step_id: Option<&str>,
    ) -> (serde_json::Value, bool) {
        let tool_start_time = chrono::Utc::now();
        let tool_name_short = tool_name
            .strip_prefix("mcp_terminator-mcp-agent_")
            .unwrap_or(tool_name);

        // Start log capture if in verbose mode
        if include_detailed {
            if let Some(ref log_capture) = self.log_capture {
                log_capture.start_capture();
            }
        }

        // The substitution is handled in `execute_sequence_impl`.
        let tool_result = self
            .dispatch_tool(peer, request_context, tool_name_short, arguments)
            .await;

        let (processed_result, error_occurred) = match tool_result {
            Ok(result) => {
                let mut extracted_content = Vec::new();

                if !result.content.is_empty() {
                    for content in &result.content {
                        match extract_content_json(content) {
                            Ok(json_content) => extracted_content.push(json_content),
                            Err(_) => extracted_content.push(
                                json!({ "type": "unknown", "data": "Content extraction failed" }),
                            ),
                        }
                    }
                }

                let content_count = result.content.len();
                let content_summary = if include_detailed {
                    // Verbose mode: include full content/step definitions
                    json!({ "type": "tool_result", "content_count": content_count, "content": extracted_content })
                } else {
                    // Normal/quiet mode: include extracted content (logs/output) but not step definitions
                    // The extracted_content already contains just the results, not the tool arguments/definitions
                    json!({
                        "type": "tool_result",
                        "status": "success",
                        "content_count": content_count,
                        "content": extracted_content
                    })
                };
                let duration_ms = (chrono::Utc::now() - tool_start_time).num_milliseconds();
                let mut result_json = json!({
                    "tool_name": tool_name,
                    "index": index,
                    "status": "success",
                    "duration_ms": duration_ms,
                    "result": content_summary,
                });

                // Add step_id if provided
                if let Some(id) = step_id {
                    if let Some(obj) = result_json.as_object_mut() {
                        obj.insert("step_id".to_string(), json!(id));
                    }
                }

                // Capture server logs if in verbose mode
                if include_detailed {
                    if let Some(ref log_capture) = self.log_capture {
                        let captured_logs = log_capture.stop_capture();
                        if !captured_logs.is_empty() {
                            if let Some(obj) = result_json.as_object_mut() {
                                obj.insert("server_logs".to_string(), json!(captured_logs));
                            }
                        }
                    }
                }

                // Extract and add logs if present (for run_command)
                if tool_name_short == "run_command" {
                    // Debug: log what's in extracted content
                    for (i, content) in extracted_content.iter().enumerate() {
                        if let Some(logs) = content.get("logs") {
                            info!(
                                "[execute_single_tool] Found logs in content[{}]: {} entries",
                                i,
                                logs.as_array().map(|a| a.len()).unwrap_or(0)
                            );
                        }
                    }

                    // Look for logs in the extracted content
                    if let Some(logs) = extracted_content
                        .iter()
                        .find_map(|c| c.get("logs").cloned())
                    {
                        info!("[execute_single_tool] Adding logs to result_json");
                        if let Some(obj) = result_json.as_object_mut() {
                            obj.insert("logs".to_string(), logs);
                        }
                    } else {
                        info!("[execute_single_tool] No logs found in extracted content");
                    }
                }

                let result_json =
                    serde_json::Value::Object(result_json.as_object().unwrap().clone());
                (result_json, false)
            }
            Err(e) => {
                // Stop log capture on error and collect logs
                let captured_logs = if include_detailed {
                    self.log_capture
                        .as_ref()
                        .map(|log_capture| log_capture.stop_capture())
                } else {
                    None
                };

                let duration_ms = (chrono::Utc::now() - tool_start_time).num_milliseconds();
                let mut error_result = json!({
                    "tool_name": tool_name,
                    "index": index,
                    "status": if is_skippable { "skipped" } else { "error" },
                    "duration_ms": duration_ms,
                    "error": format!("{}", e),
                });

                // Include server logs in error result if captured
                if let Some(logs) = captured_logs {
                    if !logs.is_empty() {
                        if let Some(obj) = error_result.as_object_mut() {
                            obj.insert("server_logs".to_string(), json!(logs));
                        }
                    }
                }

                // Add step_id if provided
                if let Some(id) = step_id {
                    if let Some(obj) = error_result.as_object_mut() {
                        obj.insert("step_id".to_string(), json!(id));
                    }
                }

                let error_result =
                    serde_json::Value::Object(error_result.as_object().unwrap().clone());

                if !is_skippable {
                    warn!(
                        "Tool '{}' at index {} failed. Reason: {}",
                        tool_name, index, e
                    );
                }
                (error_result, !is_skippable)
            }
        };

        (processed_result, error_occurred)
    }

    /// Execute TypeScript workflow
    async fn execute_typescript_workflow(
        &self,
        url: &str,
        args: ExecuteSequenceArgs,
    ) -> Result<CallToolResult, McpError> {
        info!("Executing TypeScript workflow from URL: {}", url);

        // Load saved state if resuming
        let restored_state = if args.start_from_step.is_some() {
            Self::load_workflow_state(url).await?
        } else {
            None
        };

        // Create TypeScript workflow executor
        let ts_workflow = TypeScriptWorkflow::new(url)?;

        // Execute workflow
        let result = ts_workflow
            .execute(
                args.inputs.unwrap_or(json!({})),
                args.start_from_step.as_deref(),
                args.end_at_step.as_deref(),
                restored_state,
            )
            .await?;

        // Save state for resumption
        if let Some(ref last_step_id) = result.result.last_step_id {
            Self::save_workflow_state(
                url,
                Some(last_step_id),
                result.result.last_step_index,
                &result.state,
            )
            .await?;
        }

        // Return result
        let output = json!({
            "status": result.result.status,
            "metadata": result.metadata,
            "state": result.state,
            "last_step_id": result.result.last_step_id,
            "last_step_index": result.result.last_step_index,
        });

        Ok(CallToolResult {
            content: vec![Content::text(
                serde_json::to_string_pretty(&output).unwrap(),
            )],
            is_error: Some(result.result.status != "success"),
            meta: None,
            structured_content: None,
        })
    }
}
