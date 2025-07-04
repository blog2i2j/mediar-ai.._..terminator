use anyhow::Result;
use rmcp::{schemars, schemars::JsonSchema};
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use terminator::{AutomationError, Desktop, UIElement};
use tracing::{warn, Level};
use tracing_subscriber::EnvFilter;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct EmptyArgs {}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DelayArgs {
    #[schemars(description = "Number of milliseconds to delay")]
    pub delay_ms: u64,
}

fn default_desktop() -> Arc<Desktop> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let desktop = Desktop::new(false, false).expect("Failed to create default desktop");
    #[cfg(target_os = "macos")]
    let desktop = Desktop::new(true, true).expect("Failed to create default desktop");
    Arc::new(desktop)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DesktopWrapper {
    #[serde(skip, default = "default_desktop")]
    pub desktop: Arc<Desktop>,
    #[serde(skip)]
    pub tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
}

impl Default for DesktopWrapper {
    fn default() -> Self {
        // Can't use Default::default() because we need the tool_router from the macro
        // So we'll construct it properly in server.rs
        panic!("DesktopWrapper::default() should not be used directly. Use DesktopWrapper::new() instead.");
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetWindowTreeArgs {
    #[schemars(description = "Process ID of the target application")]
    pub pid: u32,
    #[schemars(description = "Optional window title filter")]
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetFocusedWindowTreeArgs {}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetWindowsArgs {
    #[schemars(description = "Name of the application to get windows for")]
    pub app_name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetApplicationsArgs {
    #[schemars(
        description = "Whether to include the full UI tree for each application. Defaults to false."
    )]
    pub include_tree: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LocatorArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds for the action")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClickElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds for the action")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response. Defaults to true.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TypeIntoElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "The text to type into the element")]
    pub text_to_type: String,
    #[schemars(description = "Optional timeout in milliseconds for the action (default: 3000ms)")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to verify the action succeeded (default: true)")]
    pub verify_action: Option<bool>,
    #[schemars(description = "Whether to clear the element before typing (default: true)")]
    pub clear_before_typing: Option<bool>,
    #[schemars(description = "Whether to include full UI tree in the response. Defaults to true.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PressKeyArgs {
    #[schemars(description = "The key or key combination to press (e.g., 'Enter', 'Ctrl+A')")]
    pub key: String,
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds for the action")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response. Defaults to true.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GlobalKeyArgs {
    #[schemars(
        description = "The key or key combination to press (e.g., '{PageDown}', '{Ctrl}{V}')"
    )]
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunCommandArgs {
    #[schemars(description = "The command to run on Windows")]
    pub windows_command: Option<String>,
    #[schemars(description = "The command to run on Linux/macOS")]
    pub unix_command: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetClipboardArgs {
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MouseDragArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(description = "Start X coordinate")]
    pub start_x: f64,
    #[schemars(description = "Start Y coordinate")]
    pub start_y: f64,
    #[schemars(description = "End X coordinate")]
    pub end_x: f64,
    #[schemars(description = "End Y coordinate")]
    pub end_y: f64,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response (verbose mode)")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ValidateElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response (verbose mode)")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HighlightElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "BGR color code (optional, default red)")]
    pub color: Option<u32>,
    #[schemars(description = "Duration in milliseconds (optional, default 1000ms)")]
    pub duration_ms: Option<u64>,
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response (verbose mode)")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WaitForElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Condition to wait for: 'visible', 'enabled', 'focused', 'exists'")]
    pub condition: String,
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response (verbose mode)")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct NavigateBrowserArgs {
    #[schemars(description = "URL to navigate to")]
    pub url: String,
    #[schemars(description = "Optional browser name")]
    pub browser: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct OpenApplicationArgs {
    #[schemars(description = "Name of the application to open")]
    pub app_name: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClipboardArgs {
    #[schemars(description = "Text to set to clipboard")]
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SelectOptionArgs {
    #[schemars(description = "A string selector to locate the dropdown/combobox element.")]
    pub selector: String,
    #[schemars(description = "The visible text of the option to select.")]
    pub option_name: String,
    #[schemars(description = "Optional alternative selectors.")]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds.")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetToggledArgs {
    #[schemars(description = "A string selector to locate the toggleable element.")]
    pub selector: String,
    #[schemars(description = "The desired state: true for on, false for off.")]
    pub state: bool,
    #[schemars(description = "Optional alternative selectors.")]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds.")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetRangeValueArgs {
    #[schemars(description = "A string selector to locate the range-based element.")]
    pub selector: String,
    #[schemars(description = "The numerical value to set.")]
    pub value: f64,
    #[schemars(description = "Optional alternative selectors.")]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds.")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SetSelectedArgs {
    #[schemars(description = "A string selector to locate the selectable element.")]
    pub selector: String,
    #[schemars(description = "The desired state: true for selected, false for deselected.")]
    pub state: bool,
    #[schemars(description = "Optional alternative selectors.")]
    pub alternative_selectors: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds.")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[schemars(description = "Arguments for scrolling an element")]
pub struct ScrollElementArgs {
    pub selector: String,
    #[schemars(
        description = "Optional alternative selectors to try in parallel. The first selector that finds an element will be used."
    )]
    pub alternative_selectors: Option<String>,
    #[serde(default)]
    #[schemars(description = "Direction to scroll: 'up', 'down', 'left', 'right'")]
    pub direction: String,
    #[serde(default)]
    #[schemars(description = "Amount to scroll (number of lines or pages)")]
    pub amount: f64,
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response (verbose mode)")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActivateElementArgs {
    #[schemars(
        description = "A string selector to locate the element. Can be chained with ` >> `."
    )]
    pub selector: String,
    #[schemars(description = "Optional timeout in milliseconds for the action")]
    pub timeout_ms: Option<u64>,
    #[schemars(description = "Whether to include full UI tree in the response. Defaults to true.")]
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolCall {
    #[schemars(description = "The name of the tool to be executed.")]
    pub tool_name: String,
    #[schemars(description = "The arguments for the tool, as a JSON object.")]
    pub arguments: serde_json::Value,
    #[schemars(
        description = "If true, the sequence will continue even if this tool call fails. Defaults to false."
    )]
    pub continue_on_error: Option<bool>,
    #[schemars(
        description = "An optional delay in milliseconds to wait after this tool call completes."
    )]
    pub delay_ms: Option<u64>,
}

// Simplified structure for Gemini compatibility
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SequenceStep {
    #[schemars(description = "The name of the tool to execute (for single tool steps)")]
    pub tool_name: Option<String>,
    #[schemars(description = "The arguments for the tool (for single tool steps)")]
    pub arguments: Option<serde_json::Value>,
    #[schemars(description = "Continue on error flag (for single tool steps)")]
    pub continue_on_error: Option<bool>,
    #[schemars(description = "Delay after execution (for single tool steps)")]
    pub delay_ms: Option<u64>,
    #[schemars(description = "Group name (for grouped steps)")]
    pub group_name: Option<String>,
    #[schemars(description = "Steps in the group (for grouped steps)")]
    pub steps: Option<Vec<ToolCall>>,
    #[schemars(description = "Whether the group is skippable on error (for grouped steps)")]
    pub skippable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteSequenceArgs {
    #[schemars(
        description = "Array of steps to execute. Each step can be either a single tool (with tool_name and arguments) or a group (with group_name and steps)."
    )]
    pub items: Vec<SequenceStep>,
    #[schemars(description = "Whether to stop the entire sequence on first error (default: true)")]
    pub stop_on_error: Option<bool>,
    #[schemars(
        description = "Whether to include detailed results from each tool execution (default: true)"
    )]
    pub include_detailed_results: Option<bool>,
}

// Keep the old structures for internal use
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolGroup {
    pub group_name: String,
    pub steps: Vec<ToolCall>,
    pub skippable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SequenceItem {
    Tool { tool_call: ToolCall },
    Group { tool_group: ToolGroup },
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseElementArgs {
    pub selector: String,
    pub alternative_selectors: Option<String>,
    pub timeout_ms: Option<u64>,
    pub include_tree: Option<bool>,
    pub retries: Option<u32>,
}

pub fn init_logging() -> Result<()> {
    let log_level = env::var("LOG_LEVEL")
        .map(|level| match level.to_lowercase().as_str() {
            "error" => Level::ERROR,
            "warn" => Level::WARN,
            "info" => Level::INFO,
            "debug" => Level::DEBUG,
            _ => Level::INFO,
        })
        .unwrap_or(Level::INFO);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    Ok(())
}

pub fn get_timeout(timeout_ms: Option<u64>) -> Option<Duration> {
    // Default to 3 seconds instead of indefinite wait to prevent hanging
    let timeout = timeout_ms.unwrap_or(3000);
    Some(Duration::from_millis(timeout))
}

/// Try multiple selectors with primary selector priority
/// The primary selector is always preferred if it succeeds, even if alternatives also succeed
pub async fn find_element_with_fallbacks(
    desktop: &Desktop,
    primary_selector: &str,
    alternative_selectors: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<(terminator::UIElement, String), terminator::AutomationError> {
    use tokio::time::Duration;

    let timeout_duration = get_timeout(timeout_ms).unwrap_or(Duration::from_millis(3000));

    // FAST PATH: If no alternatives provided, just use primary selector directly
    if alternative_selectors.is_none() {
        let locator = desktop.locator(terminator::Selector::from(primary_selector));
        return match locator.first(Some(timeout_duration)).await {
            Ok(element) => Ok((element, primary_selector.to_string())),
            Err(e) => Err(terminator::AutomationError::ElementNotFound(format!(
                "Primary selector '{}' failed: {}",
                primary_selector, e
            ))),
        };
    }

    // Parse comma-separated alternative selectors
    let alternative_selectors_vec: Option<Vec<String>> = alternative_selectors.map(|alts| {
        alts.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    // Create primary task
    let desktop_clone = desktop.clone();
    let primary_clone = primary_selector.to_string();
    let primary_task = tokio::spawn(async move {
        let locator = desktop_clone.locator(terminator::Selector::from(primary_clone.as_str()));
        match locator.first(Some(timeout_duration)).await {
            Ok(element) => Ok((element, primary_clone)),
            Err(e) => Err((primary_clone, e)),
        }
    });

    // Create alternative tasks
    let mut alternative_tasks = Vec::new();
    if let Some(alternatives) = alternative_selectors_vec.as_ref() {
        for selector_str in alternatives {
            let desktop_clone = desktop.clone();
            let selector_clone = selector_str.clone();
            let task = tokio::spawn(async move {
                let locator =
                    desktop_clone.locator(terminator::Selector::from(selector_clone.as_str()));
                match locator.first(Some(timeout_duration)).await {
                    Ok(element) => Ok((element, selector_clone)),
                    Err(e) => Err((selector_clone, e)),
                }
            });
            alternative_tasks.push(task);
        }
    }

    // Wait for primary task first, then alternatives
    let mut errors = Vec::new();
    let mut completed_tasks = Vec::new();
    completed_tasks.push(primary_task);
    completed_tasks.extend(alternative_tasks);

    // Use select_all but prioritize primary selector if multiple succeed
    while !completed_tasks.is_empty() {
        let (result, index, remaining_tasks) = futures::future::select_all(completed_tasks).await;

        match result {
            Ok(Ok((element, selector))) => {
                // Cancel remaining tasks
                for task in remaining_tasks {
                    task.abort();
                }

                // Always prefer primary selector (index 0) if it succeeds
                if index == 0 {
                    return Ok((element, selector));
                } else {
                    // Alternative succeeded first, but give primary selector a brief grace period
                    // in case it's about to succeed too (within 10ms)
                    let desktop_clone = desktop.clone();
                    let primary_clone = primary_selector.to_string();

                    match tokio::time::timeout(Duration::from_millis(10), async move {
                        let locator = desktop_clone
                            .locator(terminator::Selector::from(primary_clone.as_str()));
                        locator.first(Some(Duration::from_millis(1))).await
                    })
                    .await
                    {
                        Ok(Ok(primary_element)) => {
                            // Primary also succeeded within grace period - prefer it
                            return Ok((primary_element, primary_selector.to_string()));
                        }
                        _ => {
                            // Primary didn't succeed quickly, use the alternative that worked
                            return Ok((element, selector));
                        }
                    }
                }
            }
            Ok(Err((selector, error))) => {
                errors.push(format!("'{}': {}", selector, error));
                completed_tasks = remaining_tasks;
            }
            Err(join_error) => {
                errors.push(format!("Task error: {}", join_error));
                completed_tasks = remaining_tasks;
            }
        }
    }

    // All selectors failed
    let combined_error = if errors.is_empty() {
        "No selectors provided".to_string()
    } else {
        format!(
            "All {} selectors failed: [{}]",
            errors.len(),
            errors.join(", ")
        )
    };

    Err(terminator::AutomationError::ElementNotFound(combined_error))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExportWorkflowSequenceArgs {
    #[schemars(
        description = "JSON value containing an array of successfully executed tool calls to convert into a reliable workflow. Each tool call should be a JSON object with 'tool_name' (string), 'arguments' (object), 'continue_on_error' (optional bool), and 'delay_ms' (optional number)."
    )]
    pub successful_tool_calls: serde_json::Value,

    #[schemars(description = "Name for the workflow being exported")]
    pub workflow_name: String,

    #[schemars(description = "Description of what this workflow accomplishes")]
    pub workflow_description: String,

    #[schemars(description = "The primary goal or intent of the workflow")]
    pub workflow_goal: String,

    #[schemars(description = "Output format: 'json' or 'yaml' (default: 'json')")]
    pub output_format: Option<String>,

    #[schemars(
        description = "Whether to include AI decision points for dynamic conditions (default: true)"
    )]
    pub include_ai_fallbacks: Option<bool>,

    #[schemars(
        description = "Whether to add extra validation steps between actions (default: true)"
    )]
    pub add_validation_steps: Option<bool>,

    #[schemars(description = "Whether to include UI tree captures at key points (default: false)")]
    pub include_tree_captures: Option<bool>,

    #[schemars(description = "Expected form data or input values used in the workflow")]
    pub expected_data: Option<serde_json::Value>,

    #[schemars(
        description = "Any credentials or login information needed (will be parameterized in output)"
    )]
    pub credentials: Option<serde_json::Value>,

    #[schemars(
        description = "Known error conditions and their solutions from the successful run as a JSON array"
    )]
    pub known_error_handlers: Option<serde_json::Value>,
}

/// A robust helper that finds a UI element and executes a provided action on it,
/// with built-in retry logic for both finding the element and performing the action.
///
/// This function is the standard way to interact with elements when reliability is key.
///
/// # Arguments
/// * `desktop` - The active `Desktop` instance.
/// * `primary_selector` - The main selector for the target element.
/// * `alternatives` - A comma-separated string of fallback selectors.
/// * `timeout_ms` - The timeout for the initial element search.
/// * `retries` - The number of times to retry the *entire find-and-act sequence*.
/// * `action` - An async closure that takes the found `UIElement` and performs an action,
///              returning a `Result`.
///
/// # Returns
/// A `Result` containing a tuple of the action's return value `T` and the `UIElement` on
/// which the action was successfully performed.
pub async fn find_and_execute_with_retry<F, Fut, T>(
    desktop: &Desktop,
    primary_selector: &str,
    alternatives: Option<&str>,
    timeout_ms: Option<u64>,
    retries: Option<u32>,
    action: F,
) -> Result<((T, UIElement), String), anyhow::Error>
where
    F: Fn(UIElement) -> Fut,
    Fut: std::future::Future<Output = Result<T, AutomationError>>,
{
    let retry_count = retries.unwrap_or(0);
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..=retry_count {
        match find_element_with_fallbacks(desktop, primary_selector, alternatives, timeout_ms).await
        {
            Ok((element, successful_selector)) => match action(element.clone()).await {
                Ok(result) => return Ok(((result, element), successful_selector)),
                Err(e) => {
                    last_error = Some(e.into());
                    if attempt < retry_count {
                        warn!(
                            "Action failed on attempt {}/{}. Retrying... Error: {}",
                            attempt + 1,
                            retry_count + 1,
                            last_error.as_ref().unwrap()
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await; // Wait before next retry
                    }
                }
            },
            Err(e) => {
                last_error = Some(e.into());
                if attempt < retry_count {
                    warn!(
                        "Find element failed on attempt {}/{}. Retrying... Error: {}",
                        attempt + 1,
                        retry_count + 1,
                        last_error.as_ref().unwrap()
                    );
                    // No need to sleep here, as find_element_with_fallbacks already has a timeout.
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow::anyhow!(
            "Action failed after {} retries for selector '{}'",
            retry_count + 1,
            primary_selector
        )
    }))
}
