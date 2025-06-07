use crate::utils::{
    get_timeout, DesktopWrapper, EmptyArgs, GetWindowTreeArgs,
    GetWindowsArgs, LocatorArgs, PressKeyArgs, RunCommandArgs, TypeIntoElementArgs,
};
use chrono::Local;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, Error as McpError, ServerHandler};
use serde_json::json;
use std::env;
use terminator::{Desktop, Locator, Selector};

#[tool(tool_box)]
impl DesktopWrapper {
    pub async fn new() -> Result<Self, McpError> {
        #[cfg(target_os = "windows")]
        let desktop = match Desktop::new(false, false) {
            Ok(d) => d,
            Err(e) => {
                return Err(McpError::internal_error(
                    "Failed to initialize terminator desktop",
                    serde_json::to_value(e.to_string()).ok(),
                ))
            }
        };

        #[cfg(target_os = "macos")]
        let desktop = match Desktop::new(true, true) {
            Ok(d) => d,
            Err(e) => {
                return Err(McpError::internal_error(
                    "Failed to initialize terminator desktop",
                    serde_json::to_value(e.to_string()).ok(),
                ))
            }
        };

        Ok(Self { desktop })
    }

    #[tool(
        description = "Get the complete UI tree for an application by PID and optional window title."
    )]
    async fn get_window_tree(
        &self,
        #[tool(param)] args: GetWindowTreeArgs,
    ) -> Result<CallToolResult, McpError> {
        let tree = self
            .desktop
            .get_window_tree(
                args.pid,
                args.title.as_deref(),
                None, // Use default config for now
            )
            .map_err(|e| {
                McpError::resource_not_found(
                    "Failed to get window tree",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

        Ok(CallToolResult::success(vec![Content::json(&tree)?]))
    }

    #[tool(description = "Get all applications currently running.")]
    async fn get_applications(
        &self,
        #[tool(param)] _args: EmptyArgs,
    ) -> Result<CallToolResult, McpError> {
        let apps = self.desktop.applications().map_err(|e| {
            McpError::resource_not_found(
                "Failed to get applications",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        let app_info: Vec<_> = apps
            .iter()
            .map(|app| {
                json!({
                    "name": app.name().unwrap_or_default(),
                    "id": app.id().unwrap_or_default(),
                    "role": app.role(),
                    "pid": app.process_id().unwrap_or(0),
                    "suggested_selector": format!("name:{}", app.name().unwrap_or_default())
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::json(&json!({
            "applications": app_info,
            "count": apps.len()
        }))?]))
    }

    #[tool(description = "Get windows for a specific application by name.")]
    async fn get_windows_for_application(
        &self,
        #[tool(param)] args: GetWindowsArgs,
    ) -> Result<CallToolResult, McpError> {
        let windows = self
            .desktop
            .windows_for_application(&args.app_name)
            .await
            .map_err(|e| {
                McpError::resource_not_found(
                    "Failed to get windows for application",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

        let window_info: Vec<_> = windows
            .iter()
            .map(|window| {
                json!({
                    "title": window.name().unwrap_or_default(),
                    "id": window.id().unwrap_or_default(),
                    "role": window.role(),
                    "bounds": window.bounds().map(|b| json!({
                        "x": b.0, "y": b.1, "width": b.2, "height": b.3
                    })).unwrap_or(json!(null)),
                    "suggested_selector": format!("name:{}", window.name().unwrap_or_default())
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::json(&json!({
            "windows": window_info,
            "count": windows.len(),
            "application": args.app_name
        }))?]))
    }

    #[tool(description = "Types text into a UI element.")]
    async fn type_into_element(
        &self,
        #[tool(param)] args: TypeIntoElementArgs,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.create_locator_for_chain(&args.selector_chain)?;
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector_chain": args.selector_chain})),
                )
            })?;
        
        // Get element details before typing for better feedback
        let element_info = json!({
            "name": element.name().unwrap_or_default(),
            "role": element.role(),
            "id": element.id().unwrap_or_default(),
            "bounds": element.bounds().map(|b| json!({
                "x": b.0, "y": b.1, "width": b.2, "height": b.3
            })).unwrap_or(json!(null)),
            "enabled": element.is_enabled().unwrap_or(false),
        });
        
        element.type_text(&args.text_to_type, false).map_err(|e| {
            McpError::resource_not_found(
                "Failed to type text",
                Some(json!({
                    "reason": e.to_string(),
                    "selector_chain": args.selector_chain,
                    "text_to_type": args.text_to_type,
                    "element_info": element_info
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::json(&json!({
            "action": "type",
            "status": "success",
            "text_typed": args.text_to_type,
            "element": element_info,
            "selector_chain": args.selector_chain,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Clicks a UI element.")]
    async fn click_element(
        &self,
        #[tool(param)] args: LocatorArgs,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.create_locator_for_chain(&args.selector_chain)?;
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector_chain": args.selector_chain})),
                )
            })?;
        
        // Get element details before clicking for better feedback
        let element_info = json!({
            "name": element.name().unwrap_or_default(),
            "role": element.role(),
            "id": element.id().unwrap_or_default(),
            "bounds": element.bounds().map(|b| json!({
                "x": b.0, "y": b.1, "width": b.2, "height": b.3
            })).unwrap_or(json!(null)),
            "enabled": element.is_enabled().unwrap_or(false),
        });
        
        element.click().map_err(|e| {
            McpError::resource_not_found(
                "Failed to click on element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector_chain": args.selector_chain,
                    "element_info": element_info
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::json(&json!({
            "action": "click",
            "status": "success",
            "element": element_info,
            "selector_chain": args.selector_chain,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Sends a key press to a UI element.")]
    async fn press_key(
        &self,
        #[tool(param)] args: PressKeyArgs,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.create_locator_for_chain(&args.selector_chain)?;
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector_chain": args.selector_chain})),
                )
            })?;
        // Get element details before pressing key for better feedback
        let element_info = json!({
            "name": element.name().unwrap_or_default(),
            "role": element.role(),
            "id": element.id().unwrap_or_default(),
            "bounds": element.bounds().map(|b| json!({
                "x": b.0, "y": b.1, "width": b.2, "height": b.3
            })).unwrap_or(json!(null)),
            "enabled": element.is_enabled().unwrap_or(false),
        });

        element.press_key(&args.key).map_err(|e| {
            McpError::resource_not_found(
                "Failed to press key",
                Some(json!({
                    "reason": e.to_string(),
                    "selector_chain": args.selector_chain,
                    "key_pressed": args.key,
                    "element_info": element_info
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::json(&json!({
            "action": "press_key",
            "status": "success",
            "key_pressed": args.key,
            "element": element_info,
            "selector_chain": args.selector_chain,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Executes a shell command.")]
    async fn run_command(
        &self,
        #[tool(param)] args: RunCommandArgs,
    ) -> Result<CallToolResult, McpError> {
        let output = self
            .desktop
            .run_command(args.unix_command.as_deref(), args.unix_command.as_deref())
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to run command",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

        Ok(CallToolResult::success(vec![Content::json(json!({
            "exit_status": output.exit_status,
            "stdout": output.stdout,
            "stderr": output.stderr,
        }))?]))
    }

    #[tool(
        description = "Activates the window containing the specified element, bringing it to the foreground."
    )]
    async fn activate_element(
        &self,
        #[tool(param)] args: LocatorArgs,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.create_locator_for_chain(&args.selector_chain)?;
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;
        element.activate_window().map_err(|e| {
            McpError::resource_not_found(
                "Failed to activate window with that element",
                Some(json!({"reason": e.to_string()})),
            )
        })?;
        Ok(CallToolResult::success(vec![Content::json(
            "Window with that element activated successfully",
        )?]))
    }

    #[tool(
        description = "Captures a screenshot of the primary monitor and returns the recognized text content (OCR)."
    )]
    async fn capture_screen(
        &self,
        #[tool(param)] _args: EmptyArgs,
    ) -> Result<CallToolResult, McpError> {
        let screenshot = self.desktop.capture_screen().await.map_err(|e| {
            McpError::internal_error(
                "Failed to capture screen",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        let ocr_text = self
            .desktop
            .ocr_screenshot(&screenshot)
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to perform OCR",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

        Ok(CallToolResult::success(vec![Content::json(&ocr_text)?]))
    }

    // keep in wrapperr to avoid creating new instance
    fn create_locator_for_chain(&self, selector_chain: &[String]) -> Result<Locator, McpError> {
        if selector_chain.is_empty() {
            return Err(McpError::invalid_params(
                "selector_chain cannot be empty",
                None,
            ));
        }

        let selectors: Vec<Selector> = selector_chain.iter().map(|s| s.as_str().into()).collect();
        let mut locator = self.desktop.locator(selectors[0].clone());

        // Chain subsequent locators
        for selector in selectors.iter().skip(1) {
            locator = locator.locator(selector.clone());
        }

        Ok(locator)
    }
}

#[tool(tool_box)]
impl ServerHandler for DesktopWrapper {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(get_server_instructions().to_string()),
        }
    }
}

fn get_server_instructions() -> String {
    let current_date_time = Local::now().to_string();
    let current_os = env::consts::OS;
    let current_working_dir = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "Unknown".to_string()); // Get current working directory

    format!(
        r#"
You are an AI assistant designed to control a computer desktop. Your primary goal is to understand the user's request and translate it into a sequence of tool calls to automate GUI interactions.

**Core Workflow: Discover, then Act**

Your most reliable strategy is to inspect the application's UI structure *before* trying to interact with it. Do not guess selectors.

1.  **Discover Running Applications:** Use `get_applications` to see what's running. This gives you the `name` and `pid` (Process ID) for each application.

2.  **Get the UI Tree:** This is the most important step. Once you have the `pid` of your target application, call `get_window_tree`. This returns a complete, JSON-like structure of all UI elements in that application.

3.  **Find Your Target Element in the Tree:** Parse the tree to locate the element you need. Each element in the tree has several properties, but you should prioritize them in this order:
    *   `id`: This is the most reliable way to find an element. It's a unique identifier.
    *   `name`: The visible text or label of the element (e.g., "Save", "File").
    *   `role`: The type of the element (e.g., "Button", "Window", "Edit").

4.  **Construct a Selector Chain:** Create a `selector_chain` (an array of strings) to target the element.
    *   **Best Practice:** Always prefer using the `id`. The selector format is `#id`. For example, if the ID is \"12345\", your selector is `#12345`.
    *   **Fallback Selectors:** If an ID is not available, you can use `name:\"Text\"` or `role:\"RoleName\"`.

5.  **Interact with the Element:** Once you have a reliable `selector_chain`, use an action tool:
    *   `click_element`: To click buttons, links, etc.
    *   `type_into_element`: To type text into input fields.
    *   `press_key`: To send special keys like 'Enter' or 'Tab'.
    *   `activate_element`: To bring a window to the foreground.

**Example Scenario:**
1.  User: \"Type \"hello\" into Notepad.\"
2.  AI: Calls `get_applications` -> Finds Notepad, gets `pid`.
3.  AI: Calls `get_window_tree` with Notepad's `pid`.
4.  AI: Looks through the tree and finds the main window element with `id: "window1"` and inside it, a text area element with `role: "Document"` and `id: "edit_pane"`.
5.  AI: Calls `type_into_element` with selector_chain: [#window1, #edit_pane] and text_to_type: hello.

**Available Tools:**

*   `get_applications`: Lists all currently running applications and their PIDs.
*   `get_window_tree`: Retrieves the entire UI element tree for an application, given its PID. **(Your primary discovery tool)**
*   `get_windows_for_application`: Get windows for a specific application by name.
*   `click_element`: Clicks a UI element specified by its `selector_chain`.
*   `type_into_element`: Types text into a UI element.
*   `press_key`: Sends a key press to a UI element.
*   `activate_element`: Brings the window containing the element to the foreground.
*   `run_command`: Executes a shell command. Use this for file operations, etc., instead of UI automation.
*   `capture_screen`: Captures the screen and performs OCR.

Contextual information:
- The current date and time is {}.
- Current operating system: {}.
- Current working directory: {}.

**Golden Rule:** Always call `get_window_tree` to understand the UI landscape before you try to act on it. Using the element `id` with a `##` prefix in your selectors will lead to the most robust automation.
"#,
        current_date_time, current_os, current_working_dir
    )
}
