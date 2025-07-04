pub use crate::utils::DesktopWrapper;
use crate::utils::{
    get_timeout, ActivateElementArgs, ClickElementArgs, ClipboardArgs, CloseElementArgs, DelayArgs,
    EmptyArgs, ExecuteSequenceArgs, ExportWorkflowSequenceArgs, GetApplicationsArgs,
    GetClipboardArgs, GetFocusedWindowTreeArgs, GetWindowTreeArgs, GetWindowsArgs, GlobalKeyArgs,
    HighlightElementArgs, LocatorArgs, MouseDragArgs, NavigateBrowserArgs, OpenApplicationArgs,
    PressKeyArgs, RunCommandArgs, ScrollElementArgs, SelectOptionArgs, SetRangeValueArgs,
    SetSelectedArgs, SetToggledArgs, ToolCall, TypeIntoElementArgs, ValidateElementArgs,
    WaitForElementArgs,
};
use chrono::Local;
use image::{ExtendedColorType, ImageEncoder};
use rmcp::handler::server::tool::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, Error as McpError, ServerHandler};
use rmcp::{tool_handler, tool_router};
use serde_json::{json, Value};
use std::env;
use std::future::Future;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;
use terminator::{Browser, Desktop, Selector, UIElement};
use tracing::warn;

// New imports for image encoding
use base64::{engine::general_purpose, Engine as _};
use image::codecs::png::PngEncoder;

/// Helper function to parse comma-separated alternative selectors into a Vec<String>
fn parse_alternative_selectors(alternatives: Option<&str>) -> Vec<String> {
    alternatives
        .map(|alts| {
            alts.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Helper function to get all selectors tried (primary + alternatives) for error reporting
fn get_selectors_tried(primary: &str, alternatives: Option<&str>) -> Vec<String> {
    let mut all = vec![primary.to_string()];
    all.extend(parse_alternative_selectors(alternatives));
    all
}

/// Builds a standardized JSON object with detailed information about a UIElement.
/// This includes a suggested selector that prioritizes role|name over just the ID.
fn build_element_info(element: &UIElement) -> Value {
    let id = element.id().unwrap_or_default();
    let role = element.role();
    let name = element.name().unwrap_or_default();

    let suggested_selector = if !name.is_empty() && role != "Unknown" {
        format!("{}|{}", &role, &name)
    } else {
        format!("#{}", id)
    };

    json!({
        "name": name,
        "role": role,
        "id": id,
        "suggested_selector": suggested_selector,
        "application": element.application_name(),
        "window_title": element.window_title(),
        "process_id": element.process_id().unwrap_or(0),
        "is_focused": element.is_focused().unwrap_or(false),
        "text": element.text(0).unwrap_or_default(),
        "bounds": element.bounds().map(|b| json!({
            "x": b.0, "y": b.1, "width": b.2, "height": b.3
        })).unwrap_or(json!(null)),
        "enabled": element.is_enabled().unwrap_or(false),
        "is_selected": element.is_selected().unwrap_or(false),
        "is_toggled": element.is_toggled().unwrap_or(false),
        "keyboard_focusable": element.is_keyboard_focusable().unwrap_or(false),
    })
}

/// Builds a standardized, actionable error when an element cannot be found.
fn build_element_not_found_error(
    primary_selector: &str,
    alternatives: Option<&str>,
    original_error: anyhow::Error,
) -> McpError {
    let selectors_tried = get_selectors_tried(primary_selector, alternatives);
    let error_payload = json!({
        "error_type": "ElementNotFound",
        "message": format!("The specified element could not be found after trying all selectors. Original error: {}", original_error),
        "selectors_tried": selectors_tried,
        "suggestions": [
            "Call `get_window_tree` again to get a fresh view of the UI; it might have changed.",
            "Verify the element's 'name' and 'role' in the new UI tree. The 'name' attribute might be empty or different from the visible text.",
            "If the element has no 'name', use its numeric ID selector (e.g., '#12345'). This is required for many clickable 'Group' elements.",
            "Use `validate_element` with your selectors to debug existence issues before calling an action tool."
        ]
    });

    McpError::resource_not_found("Element not found", Some(error_payload))
}

/// Waits for a detectable UI change after an action, like an element disappearing or focus shifting.
/// This is more efficient than a fixed sleep, as it returns as soon as a change is detected.
async fn wait_for_ui_change(
    desktop: &Desktop,
    original_element_id: &str,
    timeout: Duration,
) -> String {
    let start = tokio::time::Instant::now();

    // If the element has no unique ID, we cannot reliably track it.
    // In this case, we fall back to a brief, fixed delay.
    if original_element_id.is_empty() {
        tokio::time::sleep(Duration::from_millis(150)).await;
        return "untracked_element_clicked_fixed_delay".to_string();
    }

    let original_selector = Selector::from(format!("#{}", original_element_id).as_str());

    while start.elapsed() < timeout {
        // Check 1: Did focus change? This is often the quickest indicator.
        if let Ok(focused_element) = desktop.focused_element() {
            if focused_element.id_or_empty() != original_element_id {
                return format!("focus_changed_to: #{}", focused_element.id_or_empty());
            }
        }

        // Check 2: Did the original element disappear? (e.g., a dialog closed)
        if desktop
            .locator(original_selector.clone())
            .first(Some(Duration::from_millis(20)))
            .await
            .is_err()
        {
            return "element_disappeared".to_string();
        }

        // Yield to the scheduler and wait before the next poll.
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    "no_significant_change_detected".to_string()
}

#[tool_router]
impl DesktopWrapper {
    pub async fn new() -> Result<Self, McpError> {
        #[cfg(any(target_os = "windows", target_os = "linux"))]
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

        Ok(Self {
            desktop: Arc::new(desktop),
            tool_router: Self::tool_router(),
        })
    }

    #[tool(
        description = "Get the complete UI tree for an application by PID and optional window title. This is your primary tool for understanding the application's current state. This is a read-only operation."
    )]
    pub async fn get_window_tree(
        &self,
        Parameters(args): Parameters<GetWindowTreeArgs>,
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
                    Some(json!({"reason": e.to_string(), "pid": args.pid, "title": args.title})),
                )
            })?;

        let mut result_json = json!({
            "action": "get_window_tree",
            "status": "success",
            "pid": args.pid,
            "title": args.title,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "recommendation": "Prefer role|name selectors (e.g., 'button|Submit'). Use the element ID (e.g., '#12345') as a fallback if the name is missing or generic."
        });

        // Always include the tree unless explicitly disabled
        if let Ok(tree_val) = serde_json::to_value(tree) {
            if let Some(obj) = result_json.as_object_mut() {
                obj.insert("ui_tree".to_string(), tree_val);
            }
        }

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Get the complete UI tree for the currently focused window. This is a convenient tool that automatically detects which window has focus and returns its UI tree. This is a read-only operation."
    )]
    pub async fn get_focused_window_tree(
        &self,
        Parameters(_args): Parameters<crate::utils::GetFocusedWindowTreeArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Get the currently focused element
        let focused_element = self.desktop.focused_element().map_err(|e| {
            McpError::internal_error(
                "Failed to get focused element",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        // Get the PID and window title from the focused element
        let pid = focused_element.process_id().unwrap_or(0);

        if pid == 0 {
            return Err(McpError::internal_error(
                "Could not get process ID from focused element",
                Some(json!({"element_role": focused_element.role()})),
            ));
        }

        let window_title = focused_element.window_title();
        let app_name = focused_element.application_name();

        // Get the window tree for the focused application
        let tree = self
            .desktop
            .get_window_tree(
                pid,
                Some(&window_title),
                None, // Use default config
            )
            .map_err(|e| {
                McpError::resource_not_found(
                    "Failed to get window tree for focused window",
                    Some(json!({
                        "reason": e.to_string(),
                        "pid": pid,
                        "window_title": window_title,
                        "app_name": app_name
                    })),
                )
            })?;

        let result_json = json!({
            "action": "get_focused_window_tree",
            "status": "success",
            "focused_window": {
                "pid": pid,
                "window_title": window_title,
                "application_name": app_name,
            },
            "ui_tree": tree,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "recommendation": "Prefer role|name selectors (e.g., 'button|Submit'). Use the element ID (e.g., '#12345') as a fallback if the name is missing or generic."
        });

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Get all applications currently running and their state. This is a read-only operation."
    )]
    pub async fn get_applications(
        &self,
        Parameters(args): Parameters<GetApplicationsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let apps = self.desktop.applications().map_err(|e| {
            McpError::resource_not_found(
                "Failed to get applications",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        let include_tree = args.include_tree.unwrap_or(false);

        let app_info_futures: Vec<_> = apps
            .iter()
            .map(|app| {
                let desktop = self.desktop.clone();
                let app_name = app.name().unwrap_or_default();
                let app_id = app.id().unwrap_or_default();
                let app_role = app.role();
                let app_pid = app.process_id().unwrap_or(0);
                let is_focused = app.is_focused().unwrap_or(false);

                let suggested_selector = if !app_name.is_empty() {
                    format!("{}|{}", &app_role, &app_name)
                } else {
                    format!("#{}", app_id)
                };

                tokio::spawn(async move {
                    let tree = if include_tree && app_pid > 0 {
                        desktop.get_window_tree(app_pid, None, None).ok()
                    } else {
                        None
                    };

                    json!({
                        "name": app_name,
                        "id": app_id,
                        "role": app_role,
                        "pid": app_pid,
                        "is_focused": is_focused,
                        "suggested_selector": suggested_selector,
                        "alternative_selectors": [
                            format!("#{}", app_id),
                            format!("name:{}", app_name)
                        ],
                        "ui_tree": tree.and_then(|t| serde_json::to_value(t).ok())
                    })
                })
            })
            .collect();

        let results = futures::future::join_all(app_info_futures).await;
        let app_info: Vec<Value> = results.into_iter().filter_map(Result::ok).collect();

        Ok(CallToolResult::success(vec![Content::json(json!({
            "applications": app_info,
            "count": app_info.len(),
            "recommendation": "For applications, the name is usually reliable. For elements inside the app, prefer role|name selectors and use the ID as a fallback. Use get_window_tree with the PID for details."
        }))?]))
    }

    #[tool(description = "Get windows for a specific application by name.")]
    async fn get_windows_for_application(
        &self,
        Parameters(args): Parameters<GetWindowsArgs>,
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

        Ok(CallToolResult::success(vec![Content::json(json!({
            "windows": window_info,
            "count": windows.len(),
            "application": args.app_name
        }))?]))
    }

    #[tool(
        description = "Types text into a UI element with smart clipboard optimization and verification. Much faster than press key. This action requires the application to be focused and may change the UI."
    )]
    async fn type_into_element(
        &self,
        Parameters(args): Parameters<TypeIntoElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let text_to_type = args.text_to_type.clone();
        let should_clear = args.clear_before_typing.unwrap_or(true);

        let action = move |element: UIElement| {
            let text_to_type = text_to_type.clone();
            async move {
                if should_clear {
                    if let Err(clear_error) = element
                        .press_key("{Ctrl}a")
                        .and_then(|_| element.press_key("{Delete}"))
                    {
                        warn!(
                            "Warning: Failed to clear element before typing: {}",
                            clear_error
                        );
                    }
                }
                element.type_text(&text_to_type, true)
            }
        };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let mut result_json = json!({
            "action": "type_into_element",
            "status": "success",
            "text_typed": args.text_to_type,
            "cleared_before_typing": args.clear_before_typing.unwrap_or(true),
            "element": build_element_info(&element),
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        // Verification if requested
        if args.verify_action.unwrap_or(true) {
            // Create a new locator for verification using the successful selector
            let verification_locator = self
                .desktop
                .locator(Selector::from(successful_selector.as_str()));
            if let Ok(updated_element) = verification_locator
                .wait(Some(std::time::Duration::from_millis(500)))
                .await
            {
                let verification = json!({
                    "element_focused": updated_element.is_focused().unwrap_or(false),
                    "element_enabled": updated_element.is_enabled().unwrap_or(false),
                    "verification_timestamp": chrono::Utc::now().to_rfc3339()
                });
                if let Some(obj) = result_json.as_object_mut() {
                    obj.insert("verification".to_string(), verification);
                }
            }
        }

        // Always attach tree for better context, or if an override is provided
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            Some(element.process_id().unwrap_or(0)),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Clicks a UI element. This action requires the application to be focused and may change the UI."
    )]
    async fn click_element(
        &self,
        Parameters(args): Parameters<ClickElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((_click_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.click() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);
        let original_element_id = element.id_or_empty();

        // --- Action Consequence Verification ---
        let consequence = wait_for_ui_change(
            &self.desktop,
            &original_element_id,
            std::time::Duration::from_millis(300),
        )
        .await;

        // Build base result
        let mut result_json = json!({
            "action": "click",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "consequence": consequence
        });

        // Always attach tree for better context
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sends a key press to a UI element. Use curly brace format: '{Ctrl}c', '{Alt}{F4}', '{Enter}', '{PageDown}', etc. This action requires the application to be focused and may change the UI."
    )]
    async fn press_key(
        &self,
        Parameters(args): Parameters<PressKeyArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let key_to_press = args.key.clone();
        let action = move |element: UIElement| {
            let key_to_press = key_to_press.clone();
            async move { element.press_key(&key_to_press) }
        };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            None, // PressKey doesn't have alternative selectors yet
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(&args.selector, None, e)),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "press_key",
            "status": "success",
            "key_pressed": args.key,
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": vec![args.selector],
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(
            true, // press_key_global does not have include_tree option
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sends a key press to the currently focused element (no selector required). Use curly brace format: '{Ctrl}c', '{Alt}{F4}', '{Enter}', '{PageDown}', etc. This action requires the application to be focused and may change the UI."
    )]
    async fn press_key_global(
        &self,
        Parameters(args): Parameters<GlobalKeyArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Identify focused element
        let element = self.desktop.focused_element().map_err(|e| {
            McpError::internal_error(
                "Failed to get focused element",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        // Gather metadata for debugging / result payload
        let element_info = build_element_info(&element);

        // Perform the key press
        element.press_key(&args.key).map_err(|e| {
            McpError::resource_not_found(
                "Failed to press key on focused element",
                Some(json!({
                    "reason": e.to_string(),
                    "key_pressed": args.key,
                    "element_info": element_info
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "press_key_global",
            "status": "success",
            "key_pressed": args.key,
            "element": element_info,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(
            true, // press_key_global does not have include_tree option
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(description = "Executes a shell command.")]
    async fn run_command(
        &self,
        Parameters(args): Parameters<RunCommandArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = self
            .desktop
            .run_command(
                args.windows_command.as_deref(),
                args.unix_command.as_deref(),
            )
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
        Parameters(args): Parameters<ActivateElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            None, // ActivateElement doesn't have alternative selectors
            args.timeout_ms,
            args.retries,
            |element| async move { element.activate_window() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(&args.selector, None, e)),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "activate_element",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": vec![args.selector],
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "recommendation": "Window activated successfully. The UI tree is attached to help you find specific elements to interact with next."
        });

        // Always attach UI tree for activated elements to help with next actions
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            Some(element.process_id().unwrap_or(0)),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Captures a screenshot of the primary monitor and returns the recognized text content (OCR)."
    )]
    async fn capture_screen(
        &self,
        Parameters(_args): Parameters<EmptyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let monitor = self.desktop.get_primary_monitor().await.map_err(|e| {
            McpError::internal_error(
                "Failed to get primary monitor",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        let screenshot = self.desktop.capture_monitor(&monitor).await.map_err(|e| {
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

    #[tool(description = "Sets text to the system clipboard using shell commands.")]
    async fn set_clipboard(
        &self,
        Parameters(args): Parameters<ClipboardArgs>,
    ) -> Result<CallToolResult, McpError> {
        // todo use native clipbaord feature we implemented
        let result = if cfg!(target_os = "windows") {
            // Windows: echo "text" | clip
            let command = format!("echo \"{}\" | clip", args.text.replace("\"", "\\\""));
            self.desktop.run_command(Some(&command), None).await
        } else if cfg!(target_os = "macos") {
            // macOS: echo "text" | pbcopy
            let command = format!("echo \"{}\" | pbcopy", args.text.replace("\"", "\\\""));
            self.desktop.run_command(None, Some(&command)).await
        } else {
            // Linux: echo "text" | xclip -selection clipboard
            let command = format!(
                "echo \"{}\" | xclip -selection clipboard",
                args.text.replace("\"", "\\\"")
            );
            self.desktop.run_command(None, Some(&command)).await
        };

        result.map_err(|e| {
            McpError::internal_error(
                "Failed to set clipboard",
                Some(json!({"reason": e.to_string(), "text": args.text})),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::json(json!({
            "action": "set_clipboard",
            "status": "success",
            "text": args.text,
            "method": "shell_command",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Gets text from the system clipboard using shell commands.")]
    async fn get_clipboard(
        &self,
        Parameters(_args): Parameters<GetClipboardArgs>,
    ) -> Result<CallToolResult, McpError> {
        let command_result = if cfg!(target_os = "windows") {
            // Windows: powershell Get-Clipboard
            self.desktop
                .run_command(Some("powershell -command \"Get-Clipboard\""), None)
                .await
        } else if cfg!(target_os = "macos") {
            // macOS: pbpaste
            self.desktop.run_command(None, Some("pbpaste")).await
        } else {
            // Linux: xclip -selection clipboard -o
            self.desktop
                .run_command(None, Some("xclip -selection clipboard -o"))
                .await
        };

        match command_result {
            Ok(output) => Ok(CallToolResult::success(vec![Content::json(json!({
                "action": "get_clipboard",
                "status": "success",
                "text": output.stdout.trim(),
                "method": "shell_command",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))?])),
            Err(e) => Err(McpError::internal_error(
                "Failed to get clipboard text",
                Some(json!({"reason": e.to_string()})),
            )),
        }
    }

    #[tool(
        description = "Delays execution for a specified number of milliseconds. Useful for waiting between actions to ensure UI stability."
    )]
    async fn delay(
        &self,
        Parameters(args): Parameters<DelayArgs>,
    ) -> Result<CallToolResult, McpError> {
        let start_time = chrono::Utc::now();

        // Use tokio's sleep for async delay
        tokio::time::sleep(std::time::Duration::from_millis(args.delay_ms)).await;

        let end_time = chrono::Utc::now();
        let actual_delay_ms = (end_time - start_time).num_milliseconds();

        Ok(CallToolResult::success(vec![Content::json(json!({
            "action": "delay",
            "status": "success",
            "requested_delay_ms": args.delay_ms,
            "actual_delay_ms": actual_delay_ms,
            "timestamp": end_time.to_rfc3339()
        }))?]))
    }

    #[tool(
        description = "Performs a mouse drag operation from start to end coordinates. This action requires the application to be focused and may change the UI."
    )]
    async fn mouse_drag(
        &self,
        Parameters(args): Parameters<MouseDragArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let action = |element: UIElement| async move {
            element.mouse_drag(args.start_x, args.start_y, args.end_x, args.end_y)
        };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            None, // MouseDrag doesn't have alternative selectors
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(&args.selector, None, e)),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "mouse_drag",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": vec![args.selector],
            "start": (args.start_x, args.start_y),
            "end": (args.end_x, args.end_y),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Validates that an element exists and provides detailed information about it. This is a read-only operation."
    )]
    pub async fn validate_element(
        &self,
        Parameters(args): Parameters<ValidateElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        // For validation, the "action" is just succeeding.
        let action = |element: UIElement| async move { Ok(element) };

        match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((element, _), successful_selector)) => {
                let mut element_info = build_element_info(&element);
                if let Some(obj) = element_info.as_object_mut() {
                    obj.insert("exists".to_string(), json!(true));
                }

                let mut result_json = json!({
                    "action": "validate_element",
                    "status": "success",
                    "element": element_info,
                    "selector_used": successful_selector,
                    "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
                    "timestamp": chrono::Utc::now().to_rfc3339()
                });
                self.maybe_attach_tree(
                    args.include_tree.unwrap_or(true),
                    element.process_id().ok(),
                    &mut result_json,
                );

                Ok(CallToolResult::success(vec![Content::json(result_json)?]))
            }
            Err(e) => {
                let selectors_tried =
                    get_selectors_tried(&args.selector, args.alternative_selectors.as_deref());
                let reason_payload = json!({
                    "error_type": "ElementNotFound",
                    "message": format!("The specified element could not be found after trying all selectors. Original error: {}", e),
                    "selectors_tried": selectors_tried,
                    "suggestions": [
                        "Call `get_window_tree` again to get a fresh view of the UI; it might have changed.",
                        "Verify the element's 'name' and 'role' in the new UI tree. The 'name' attribute might be empty or different from the visible text.",
                        "If the element has no 'name', use its numeric ID selector (e.g., '#12345')."
                    ]
                });

                // This is not a tool error, but a validation failure, so we return success with the failure info.
                Ok(CallToolResult::success(vec![Content::json(json!({
                    "action": "validate_element",
                    "status": "failed",
                    "exists": false,
                    "reason": reason_payload,
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))?]))
            }
        }
    }

    #[tool(description = "Highlights an element with a colored border for visual confirmation.")]
    async fn highlight_element(
        &self,
        Parameters(args): Parameters<HighlightElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;
        let duration = args.duration_ms.map(std::time::Duration::from_millis);
        let color = args.color;

        let action = |element: UIElement| async move { element.highlight(color, duration) };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "highlight_element",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "color": args.color.unwrap_or(0x0000FF),
            "duration_ms": args.duration_ms.unwrap_or(1000),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Waits for an element to meet a specific condition (visible, enabled, focused, exists)."
    )]
    async fn wait_for_element(
        &self,
        Parameters(args): Parameters<WaitForElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let timeout = get_timeout(args.timeout_ms);

        // Call the underlying wait function once and store its result.
        let wait_result = locator.wait(timeout).await;

        let (condition_met, maybe_element) = match wait_result {
            Ok(element) => {
                // Wait succeeded, now check the specific condition.
                let condition_lower = args.condition.to_lowercase();
                let met = match condition_lower.as_str() {
                    "exists" => Ok(true),
                    "visible" => element.is_visible(),
                    "enabled" => element.is_enabled(),
                    "focused" => element.is_focused(),
                    _ => {
                        return Err(McpError::invalid_params(
                            "Invalid condition. Valid: exists, visible, enabled, focused",
                            Some(json!({"provided_condition": args.condition})),
                        ))
                    }
                }
                .unwrap_or(false); // Default to false on property check error

                (met, Some(element))
            }
            Err(_) => {
                // If the element was not found, no condition can be met.
                (false, None)
            }
        };

        // Build the result payload.
        let mut result_json = json!({
            "action": "wait_for_element",
            "status": "success",
            "condition": args.condition,
            "condition_met": condition_met,
            "selector": args.selector,
            "timeout_ms": args.timeout_ms.unwrap_or(5000),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        // Conditionally attach the UI tree if requested and an element was found.
        if let Some(element) = maybe_element {
            self.maybe_attach_tree(
                args.include_tree.unwrap_or(false),
                element.process_id().ok(),
                &mut result_json,
            );
        }

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Opens a URL in the specified browser (uses SDK's built-in browser automation)."
    )]
    async fn navigate_browser(
        &self,
        Parameters(args): Parameters<NavigateBrowserArgs>,
    ) -> Result<CallToolResult, McpError> {
        let browser = args.browser.clone().map(Browser::Custom);
        let ui_element = self.desktop.open_url(&args.url, browser).map_err(|e| {
            McpError::internal_error(
                "Failed to open URL",
                Some(json!({"reason": e.to_string(), "url": args.url, "browser": args.browser})),
            )
        })?;

        let element_info = build_element_info(&ui_element);

        let tree = self
            .desktop
            .get_window_tree(
                ui_element.process_id().unwrap_or(0),
                ui_element.name().as_deref(),
                None,
            )
            .unwrap_or_default();

        Ok(CallToolResult::success(vec![Content::json(json!({
            "action": "navigate_browser",
            "status": "success",
            "url": args.url,
            "browser": args.browser,
            "element": element_info,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "ui_tree": tree
        }))?]))
    }

    #[tool(description = "Opens an application by name (uses SDK's built-in app launcher).")]
    async fn open_application(
        &self,
        Parameters(args): Parameters<OpenApplicationArgs>,
    ) -> Result<CallToolResult, McpError> {
        let ui_element = self.desktop.open_application(&args.app_name).map_err(|e| {
            McpError::internal_error(
                "Failed to open application",
                Some(json!({"reason": e.to_string(), "app_name": args.app_name})),
            )
        })?;

        let process_id = ui_element.process_id().unwrap_or(0);
        let window_title = ui_element.window_title();

        let element_info = build_element_info(&ui_element);

        let mut result_json = json!({
            "action": "open_application",
            "status": "success",
            "app_name": args.app_name,
            "application": element_info,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "recommendation": "Application opened successfully. Use get_window_tree with the PID to get the full UI structure for reliable element targeting."
        });

        // Always attach the full UI tree for newly opened applications
        if process_id > 0 {
            if let Ok(tree) =
                self.desktop
                    .get_window_tree(process_id, Some(window_title.as_str()), None)
            {
                if let Ok(tree_val) = serde_json::to_value(tree) {
                    if let Some(obj) = result_json.as_object_mut() {
                        obj.insert("ui_tree".to_string(), tree_val);
                    }
                }
            }
        }

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Closes a UI element (window, application, dialog, etc.) if it's closable."
    )]
    async fn close_element(
        &self,
        Parameters(args): Parameters<CloseElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.close() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        Ok(CallToolResult::success(vec![Content::json(json!({
            "action": "close_element",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Scrolls a UI element in the specified direction by the given amount.")]
    async fn scroll_element(
        &self,
        Parameters(args): Parameters<ScrollElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let direction = args.direction.clone();
        let amount = args.amount;
        let action = move |element: UIElement| {
            let direction = direction.clone();
            async move { element.scroll(&direction, amount) }
        };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "scroll_element",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "direction": args.direction,
            "amount": args.amount,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(description = "Selects an option in a dropdown or combobox by its visible text.")]
    async fn select_option(
        &self,
        Parameters(args): Parameters<SelectOptionArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let option_name = args.option_name.clone();
        let action = move |element: UIElement| {
            let option_name = option_name.clone();
            async move { element.select_option(&option_name) }
        };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "select_option",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "option_selected": args.option_name,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Lists all available option strings from a dropdown, list box, or similar control. This is a read-only operation."
    )]
    async fn list_options(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((options, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.list_options() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "list_options",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "options": options,
            "count": options.len(),
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the state of a toggleable control (e.g., checkbox, switch). This action requires the application to be focused and may change the UI."
    )]
    async fn set_toggled(
        &self,
        Parameters(args): Parameters<SetToggledArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let state = args.state;
        let action = move |element: UIElement| async move { element.set_toggled(state) };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "set_toggled",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "state_set_to": args.state,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            Some(element.process_id().unwrap_or(0)),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the value of a range-based control like a slider. This action requires the application to be focused and may change the UI."
    )]
    async fn set_range_value(
        &self,
        Parameters(args): Parameters<SetRangeValueArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let value = args.value;
        let action = move |element: UIElement| async move { element.set_range_value(value) };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "set_range_value",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "value_set_to": args.value,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            Some(element.process_id().unwrap_or(0)),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the selection state of a selectable item (e.g., in a list or calendar). This action requires the application to be focused and may change the UI."
    )]
    async fn set_selected(
        &self,
        Parameters(args): Parameters<SetSelectedArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let state = args.state;
        let action = move |element: UIElement| async move { element.set_selected(state) };

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            action,
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "set_selected",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "state_set_to": args.state,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            Some(element.process_id().unwrap_or(0)),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Checks if a control (like a checkbox or toggle switch) is currently toggled on. This is a read-only operation."
    )]
    async fn is_toggled(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((is_toggled, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.is_toggled() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "is_toggled",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "is_toggled": is_toggled,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Gets the current value from a range-based control like a slider or progress bar. This is a read-only operation."
    )]
    async fn get_range_value(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((value, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.get_range_value() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "get_range_value",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "value": value,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Checks if a selectable item (e.g., in a calendar, list, or tab) is currently selected. This is a read-only operation."
    )]
    async fn is_selected(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((is_selected, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.is_selected() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "is_selected",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "is_selected": is_selected,
        });
        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(description = "Captures a screenshot of a specific UI element.")]
    async fn capture_element_screenshot(
        &self,
        Parameters(args): Parameters<ValidateElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((screenshot_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.capture() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let mut png_data = Vec::new();
        let encoder = PngEncoder::new(Cursor::new(&mut png_data));
        encoder
            .write_image(
                &screenshot_result.image_data,
                screenshot_result.width,
                screenshot_result.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to encode screenshot to PNG",
                    Some(json!({ "reason": e.to_string() })),
                )
            })?;

        let base64_image = general_purpose::STANDARD.encode(&png_data);

        let element_info = build_element_info(&element);

        Ok(CallToolResult::success(vec![
            Content::json(json!({
                "action": "capture_element_screenshot",
                "status": "success",
                "element": element_info,
                "selector_used": successful_selector,
                "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
                "image_format": "png",
            }))?,
            Content::image(base64_image, "image/png".to_string()),
        ]))
    }

    #[tool(
        description = "Invokes a UI element. This is often more reliable than clicking for controls like radio buttons or menu items. This action requires the application to be focused and may change the UI."
    )]
    async fn invoke_element(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_and_execute_with_retry;

        let ((_result, element), successful_selector) = match find_and_execute_with_retry(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
            args.retries,
            |element| async move { element.invoke() },
        )
        .await
        {
            Ok(((result, element), selector)) => Ok(((result, element), selector)),
            Err(e) => Err(build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e,
            )),
        }?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "invoke",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        self.maybe_attach_tree(
            args.include_tree.unwrap_or(true),
            element.process_id().ok(),
            &mut result_json,
        );

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Executes multiple tools in sequence. Useful for automating complex workflows that require multiple steps. Each tool in the sequence can have its own error handling and delay configuration. Tool names can be provided either in short form (e.g., 'click_element') or full form (e.g., 'mcp_terminator-mcp-agent_click_element')."
    )]
    pub async fn execute_sequence(
        &self,
        Parameters(args): Parameters<ExecuteSequenceArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::{SequenceItem, ToolCall, ToolGroup};

        let stop_on_error = args.stop_on_error.unwrap_or(true);
        let include_detailed = args.include_detailed_results.unwrap_or(true);

        // Convert flattened SequenceStep to internal SequenceItem representation
        let mut sequence_items = Vec::new();
        for step in args.items {
            if let Some(tool_name) = step.tool_name {
                // This is a single tool step
                let tool_call = ToolCall {
                    tool_name,
                    arguments: step.arguments.unwrap_or(serde_json::json!({})),
                    continue_on_error: step.continue_on_error,
                    delay_ms: step.delay_ms,
                };
                sequence_items.push(SequenceItem::Tool { tool_call });
            } else if let Some(group_name) = step.group_name {
                // This is a group step
                let tool_group = ToolGroup {
                    group_name,
                    steps: step.steps.unwrap_or_default(),
                    skippable: step.skippable,
                };
                sequence_items.push(SequenceItem::Group { tool_group });
            } else {
                return Err(McpError::invalid_params(
                    "Each step must have either tool_name (for single tools) or group_name (for groups)",
                    Some(json!({"invalid_step": step})),
                ));
            }
        }

        let mut results = Vec::new();
        let mut sequence_had_errors = false;
        let mut sequence_should_stop = false;
        let start_time = chrono::Utc::now();

        for (item_index, item) in sequence_items.iter().enumerate() {
            match item {
                SequenceItem::Tool { tool_call } => {
                    let (result, error_occurred) = self
                        .execute_single_tool(tool_call, item_index, include_detailed)
                        .await;

                    if result["status"] != "success" {
                        sequence_had_errors = true;
                    }

                    if error_occurred {
                        sequence_should_stop = true;
                    }

                    results.push(result);

                    if sequence_should_stop && stop_on_error {
                        break;
                    }
                }
                SequenceItem::Group { tool_group } => {
                    let mut group_had_errors = false;
                    let mut group_results = Vec::new();
                    let is_skippable = tool_group.skippable.unwrap_or(false);
                    for (step_index, tool_call) in tool_group.steps.iter().enumerate() {
                        let (result, error_occurred) = self
                            .execute_single_tool(tool_call, step_index, include_detailed)
                            .await;

                        group_results.push(result.clone());

                        let tool_failed = result["status"] != "success";
                        if tool_failed {
                            group_had_errors = true;

                            // A tool failed. Check if we should break out of the group.
                            // We break if the error is fatal (error_occurred) OR if the group
                            // is designated as skippable (meaning we abandon it on any failure).
                            if error_occurred || is_skippable {
                                // If the error was fatal AND the group was NOT skippable,
                                // the entire sequence must stop.
                                if error_occurred && !is_skippable {
                                    sequence_should_stop = true;
                                }
                                break; // Exit the loop for this group's steps.
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
                    }

                    if group_had_errors && !is_skippable && stop_on_error {
                        sequence_should_stop = true;
                    }

                    results.push(json!({
                        "group_name": tool_group.group_name,
                        "status": group_status,
                        "results": group_results
                    }));

                    if sequence_should_stop && stop_on_error {
                        break;
                    }
                }
            }
        }

        let total_duration = (chrono::Utc::now() - start_time).num_milliseconds();

        let final_status = if !sequence_had_errors {
            "success"
        } else if sequence_should_stop {
            "partial_success"
        } else {
            "completed_with_errors"
        };

        let summary = json!({
            "action": "execute_sequence",
            "status": final_status,
            "total_tools": sequence_items.len(),
            "executed_tools": results.len(),
            "total_duration_ms": total_duration,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "results": results,
        });

        Ok(CallToolResult::success(vec![Content::json(summary)?]))
    }

    async fn execute_single_tool(
        &self,
        tool_call: &crate::utils::ToolCall,
        index: usize,
        include_detailed: bool,
    ) -> (serde_json::Value, bool) {
        let tool_start_time = chrono::Utc::now();
        let tool_name = tool_call
            .tool_name
            .strip_prefix("mcp_terminator-mcp-agent_")
            .unwrap_or(&tool_call.tool_name);

        let tool_result = self.dispatch_tool(tool_name, &tool_call.arguments).await;

        let (processed_result, error_occurred) = match tool_result {
            Ok(result) => {
                let mut extracted_content = Vec::new();
                for content in &result.content {
                    if let Ok(json_content) = serde_json::to_value(content) {
                        extracted_content.push(json_content);
                    } else {
                        extracted_content.push(
                            json!({ "type": "unknown", "data": "Content extraction failed" }),
                        );
                    }
                }
                let content_summary = if include_detailed {
                    json!({ "type": "tool_result", "content_count": result.content.len(), "content": extracted_content })
                } else {
                    json!({ "type": "summary", "content": "Tool executed successfully", "content_count": result.content.len() })
                };
                let duration_ms = (chrono::Utc::now() - tool_start_time).num_milliseconds();
                let result_json = json!({
                    "tool_name": tool_call.tool_name,
                    "index": index,
                    "status": "success",
                    "duration_ms": duration_ms,
                    "result": content_summary,
                });
                (result_json, false)
            }
            Err(e) => {
                let duration_ms = (chrono::Utc::now() - tool_start_time).num_milliseconds();
                let is_skippable_error = tool_call.continue_on_error.unwrap_or(false);
                let error_result = json!({
                    "tool_name": tool_call.tool_name,
                    "index": index,
                    "status": if is_skippable_error { "skipped" } else { "error" },
                    "duration_ms": duration_ms,
                    "error": e.to_string(),
                });

                if !is_skippable_error {
                    warn!(
                        "Tool '{}' at index {} failed. Reason: {}",
                        tool_call.tool_name, index, e
                    );
                }
                (error_result, !is_skippable_error)
            }
        };

        if let Some(delay_ms) = tool_call.delay_ms {
            if delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
        (processed_result, error_occurred)
    }

    async fn dispatch_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<CallToolResult, McpError> {
        use rmcp::handler::server::tool::Parameters;
        match tool_name {
            "get_window_tree" => {
                match serde_json::from_value::<GetWindowTreeArgs>(arguments.clone()) {
                    Ok(args) => self.get_window_tree(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for get_window_tree",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "get_focused_window_tree" => {
                match serde_json::from_value::<GetFocusedWindowTreeArgs>(arguments.clone()) {
                    Ok(args) => self.get_focused_window_tree(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for get_focused_window_tree",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "get_applications" => {
                match serde_json::from_value::<GetApplicationsArgs>(arguments.clone()) {
                    Ok(args) => self.get_applications(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for get_applications",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "click_element" => {
                match serde_json::from_value::<ClickElementArgs>(arguments.clone()) {
                    Ok(args) => self.click_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for click_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "type_into_element" => {
                match serde_json::from_value::<TypeIntoElementArgs>(arguments.clone()) {
                    Ok(args) => self.type_into_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for type_into_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "press_key" => match serde_json::from_value::<PressKeyArgs>(arguments.clone()) {
                Ok(args) => self.press_key(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for press_key",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "press_key_global" => {
                match serde_json::from_value::<GlobalKeyArgs>(arguments.clone()) {
                    Ok(args) => self.press_key_global(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for press_key_global",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "validate_element" => {
                match serde_json::from_value::<ValidateElementArgs>(arguments.clone()) {
                    Ok(args) => self.validate_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for validate_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "wait_for_element" => {
                match serde_json::from_value::<WaitForElementArgs>(arguments.clone()) {
                    Ok(args) => self.wait_for_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for wait_for_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "activate_element" => {
                match serde_json::from_value::<ActivateElementArgs>(arguments.clone()) {
                    Ok(args) => self.activate_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for activate_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "navigate_browser" => {
                match serde_json::from_value::<NavigateBrowserArgs>(arguments.clone()) {
                    Ok(args) => self.navigate_browser(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for navigate_browser",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "open_application" => {
                match serde_json::from_value::<OpenApplicationArgs>(arguments.clone()) {
                    Ok(args) => self.open_application(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for open_application",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "scroll_element" => {
                match serde_json::from_value::<ScrollElementArgs>(arguments.clone()) {
                    Ok(args) => self.scroll_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for scroll_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "set_clipboard" => match serde_json::from_value::<ClipboardArgs>(arguments.clone()) {
                Ok(args) => self.set_clipboard(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for set_clipboard",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "get_clipboard" => {
                match serde_json::from_value::<GetClipboardArgs>(arguments.clone()) {
                    Ok(args) => self.get_clipboard(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for get_clipboard",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "delay" => match serde_json::from_value::<DelayArgs>(arguments.clone()) {
                Ok(args) => self.delay(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for delay",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "get_windows_for_application" => {
                match serde_json::from_value::<GetWindowsArgs>(arguments.clone()) {
                    Ok(args) => self.get_windows_for_application(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for get_windows_for_application",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "run_command" => match serde_json::from_value::<RunCommandArgs>(arguments.clone()) {
                Ok(args) => self.run_command(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for run_command",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "capture_screen" => match serde_json::from_value::<EmptyArgs>(arguments.clone()) {
                Ok(args) => self.capture_screen(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for capture_screen",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "mouse_drag" => match serde_json::from_value::<MouseDragArgs>(arguments.clone()) {
                Ok(args) => self.mouse_drag(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for mouse_drag",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "highlight_element" => {
                match serde_json::from_value::<HighlightElementArgs>(arguments.clone()) {
                    Ok(args) => self.highlight_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for highlight_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "close_element" => {
                match serde_json::from_value::<CloseElementArgs>(arguments.clone()) {
                    Ok(args) => self.close_element(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for close_element",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "select_option" => {
                match serde_json::from_value::<SelectOptionArgs>(arguments.clone()) {
                    Ok(args) => self.select_option(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for select_option",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "list_options" => match serde_json::from_value::<LocatorArgs>(arguments.clone()) {
                Ok(args) => self.list_options(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for list_options",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "set_toggled" => match serde_json::from_value::<SetToggledArgs>(arguments.clone()) {
                Ok(args) => self.set_toggled(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for set_toggled",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "set_range_value" => {
                match serde_json::from_value::<SetRangeValueArgs>(arguments.clone()) {
                    Ok(args) => self.set_range_value(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for set_range_value",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "set_selected" => match serde_json::from_value::<SetSelectedArgs>(arguments.clone()) {
                Ok(args) => self.set_selected(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for set_selected",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "is_toggled" => match serde_json::from_value::<LocatorArgs>(arguments.clone()) {
                Ok(args) => self.is_toggled(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for is_toggled",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "get_range_value" => match serde_json::from_value::<LocatorArgs>(arguments.clone()) {
                Ok(args) => self.get_range_value(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for get_range_value",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "is_selected" => match serde_json::from_value::<LocatorArgs>(arguments.clone()) {
                Ok(args) => self.is_selected(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for is_selected",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            "capture_element_screenshot" => {
                match serde_json::from_value::<ValidateElementArgs>(arguments.clone()) {
                    Ok(args) => self.capture_element_screenshot(Parameters(args)).await,
                    Err(e) => Err(McpError::invalid_params(
                        "Invalid arguments for capture_element_screenshot",
                        Some(json!({"error": e.to_string()})),
                    )),
                }
            }
            "invoke_element" => match serde_json::from_value::<LocatorArgs>(arguments.clone()) {
                Ok(args) => self.invoke_element(Parameters(args)).await,
                Err(e) => Err(McpError::invalid_params(
                    "Invalid arguments for invoke_element",
                    Some(json!({"error": e.to_string()})),
                )),
            },
            _ => Err(McpError::internal_error(
                "Unknown tool called",
                Some(json!({"tool_name": tool_name})),
            )),
        }
    }

    #[tool(
        description = "Exports a sequence of successful tool calls into a structured, reliable workflow format that can be executed by another AI agent with minimal context. This tool analyzes the provided sequence and enhances it with intelligent error handling, validation steps, wait conditions, and fallback strategies to maximize success rate. The output can be in JSON or YAML format and includes comprehensive metadata to ensure reproducibility."
    )]
    pub async fn export_workflow_sequence(
        &self,
        Parameters(args): Parameters<ExportWorkflowSequenceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output_format = args.output_format.clone().unwrap_or("json".to_string());
        let include_ai_fallbacks = args.include_ai_fallbacks.unwrap_or(true);
        let add_validation_steps = args.add_validation_steps.unwrap_or(true);
        let include_tree_captures = args.include_tree_captures.unwrap_or(false);

        // Parse the JSON value as an array of tool calls
        let tool_calls_array: Vec<serde_json::Value> = serde_json::from_value(args.successful_tool_calls.clone())
            .map_err(|e| McpError::invalid_params(
                "successful_tool_calls must be a JSON array",
                Some(json!({
                    "error": e.to_string(),
                    "expected": "JSON array of tool call objects",
                    "example": "[{\"tool_name\": \"click_element\", \"arguments\": {\"selector\": \"#button\"}}]"
                })),
            ))?;

        // Parse the JSON values into ToolCall structs
        let mut parsed_tool_calls = Vec::new();
        for (index, json_value) in tool_calls_array.iter().enumerate() {
            match serde_json::from_value::<ToolCall>(json_value.clone()) {
                Ok(tool_call) => parsed_tool_calls.push(tool_call),
                Err(e) => {
                    return Err(McpError::invalid_params(
                        "Invalid tool call format",
                        Some(json!({
                            "error": e.to_string(),
                            "index": index,
                            "expected_format": {
                                "tool_name": "string",
                                "arguments": "object",
                                "continue_on_error": "optional bool",
                                "delay_ms": "optional number"
                            }
                        })),
                    ))
                }
            }
        }

        // Build the workflow steps with enhancements
        let mut enhanced_steps = Vec::new();
        let mut step_counter = 1;

        for (index, tool_call) in parsed_tool_calls.iter().enumerate() {
            // Analyze the tool to determine what enhancements to add
            let tool_name = &tool_call.tool_name;

            // Add focus check before UI interaction tools
            if matches!(
                tool_name.as_str(),
                "click_element"
                    | "type_into_element"
                    | "press_key"
                    | "invoke_element"
                    | "select_option"
            ) && (index == 0 || should_add_focus_check(&parsed_tool_calls, index))
            {
                enhanced_steps.push(json!({
                    "step": step_counter,
                    "action": "validate_focus",
                    "description": "Ensure the target application has focus",
                    "tool_name": "get_applications",
                    "condition": "Check if target app is_focused=true",
                    "fallback": "Use activate_element if not focused"
                }));
                step_counter += 1;
            }

            // Add wait after navigation or state-changing actions
            if matches!(tool_name.as_str(), "navigate_browser" | "open_application") {
                enhanced_steps.push(json!({
                    "step": step_counter,
                    "action": tool_name,
                    "description": tool_call.arguments.get("description").and_then(|v| v.as_str()).unwrap_or("Execute action"),
                    "tool_name": tool_name,
                    "arguments": tool_call.arguments.clone(),
                    "success_criteria": "Page/App loads successfully"
                }));
                step_counter += 1;

                // Add intelligent wait
                enhanced_steps.push(json!({
                    "step": step_counter,
                    "action": "wait_for_stability",
                    "description": "Wait for UI to stabilize after navigation",
                    "tool_name": "wait_for_element",
                    "arguments": {
                        "selector": "role:Document",
                        "condition": "exists",
                        "timeout_ms": 5000
                    },
                    "fallback": "If timeout, check get_window_tree for current state"
                }));
                step_counter += 1;
            } else {
                // Process the actual tool call with enhancements
                let mut enhanced_args = tool_call.arguments.clone();

                // Extract selectors and add alternatives if available
                if let Some(_selector) =
                    tool_call.arguments.get("selector").and_then(|v| v.as_str())
                {
                    // Look for alternative selectors from the arguments
                    if let Some(alternatives) = tool_call.arguments.get("alternative_selectors") {
                        enhanced_args["alternative_selectors"] = alternatives.clone();
                    }
                }

                enhanced_steps.push(json!({
                    "step": step_counter,
                    "action": tool_name,
                    "description": self.generate_step_description(tool_name, &tool_call.arguments),
                    "tool_name": tool_name,
                    "arguments": enhanced_args,
                    "wait_for": self.get_wait_condition(tool_name),
                    "verify_success": add_validation_steps
                }));
                step_counter += 1;
            }

            // Add validation after state-changing actions if requested
            if add_validation_steps && is_state_changing_action(tool_name) {
                if let Some(selector) = tool_call.arguments.get("selector") {
                    enhanced_steps.push(json!({
                        "step": step_counter,
                        "action": "validate_action_result",
                        "description": format!("Verify {} completed successfully", tool_name),
                        "tool_name": "validate_element",
                        "arguments": {
                            "selector": selector,
                            "timeout_ms": 1000
                        },
                        "condition": "Element still exists and state changed as expected"
                    }));
                    step_counter += 1;
                }
            }

            // Add tree capture at key points if requested
            if include_tree_captures
                && should_capture_tree(tool_name, index, parsed_tool_calls.len())
            {
                enhanced_steps.push(json!({
                    "step": step_counter,
                    "action": "capture_ui_state",
                    "description": "Capture UI tree for debugging/verification",
                    "tool_name": "get_window_tree",
                    "arguments": {
                        "include_tree": true
                    },
                    "purpose": "State checkpoint for recovery"
                }));
                step_counter += 1;
            }
        }

        // Build the complete workflow structure
        let workflow = json!({
            "workflow": {
                "name": args.workflow_name,
                "version": "1.0",
                "description": args.workflow_description,
                "goal": args.workflow_goal,
                "created_at": chrono::Utc::now().to_rfc3339(),
                "created_by": "terminator-mcp-agent",

                "prerequisites": {
                    "browser": "Chrome",
                    "platform": env::consts::OS,
                    "required_tools": self.extract_required_tools(&parsed_tool_calls)
                },

                "parameters": {
                    "credentials": args.credentials.unwrap_or(json!({})),
                    "form_data": args.expected_data.unwrap_or(json!({}))
                },

                "configuration": {
                    "include_ai_fallbacks": include_ai_fallbacks,
                    "add_validation_steps": add_validation_steps,
                    "default_timeout_ms": 3000,
                    "retry_on_failure": true,
                    "max_retries": 2
                },

                "steps": enhanced_steps,

                "error_handling": {
                    "known_errors": args.known_error_handlers.unwrap_or(json!([])),
                    "general_strategies": [
                        {
                            "error": "ElementNotFound",
                            "solution": "Call get_window_tree to refresh UI state, then retry with alternative selectors"
                        },
                        {
                            "error": "ElementDisabled",
                            "solution": "Check prerequisites - ensure all required fields are filled and conditions met"
                        },
                        {
                            "error": "Timeout",
                            "solution": "Increase timeout_ms or add explicit wait_for_element steps"
                        }
                    ]
                },

                "success_criteria": {
                    "final_validation": "Verify the workflow goal has been achieved",
                    "expected_outcomes": self.infer_expected_outcomes(&parsed_tool_calls),
                    "verification_steps": if add_validation_steps {
                        vec!["Check final UI state matches expected", "Verify data was processed correctly"]
                    } else {
                        vec![]
                    }
                },

                "ai_decision_points": if include_ai_fallbacks {
                    json!([
                        {
                            "condition": "Dialog or popup appears unexpectedly",
                            "action": "Analyze dialog content and decide whether to accept, cancel, or handle differently"
                        },
                        {
                            "condition": "Expected element not found after multiple retries",
                            "action": "Use get_window_tree to understand current state and find alternative path"
                        },
                        {
                            "condition": "Form validation errors",
                            "action": "Read error messages and adjust input data accordingly"
                        }
                    ])
                } else {
                    json!([])
                },

                "notes": [
                    "This workflow was automatically generated from successful tool executions",
                    "Selectors use exact IDs where possible for maximum reliability",
                    "Alternative selectors are included for robustness",
                    "Wait conditions and validations ensure each step completes before proceeding"
                ]
            }
        });

        // Convert to requested format
        let output = match output_format.to_lowercase().as_str() {
            "yaml" => {
                // For YAML output, we'll return instructions since we can't directly convert
                json!({
                    "format": "yaml",
                    "content": workflow,
                    "note": "Copy the 'content' field and convert to YAML using a JSON-to-YAML converter for proper formatting"
                })
            }
            _ => workflow,
        };

        Ok(CallToolResult::success(vec![Content::json(output)?]))
    }

    // Helper methods for export_workflow_sequence
    fn generate_step_description(&self, tool_name: &str, args: &serde_json::Value) -> String {
        match tool_name {
            "click_element" => format!(
                "Click on element: {}",
                args.get("selector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            ),
            "type_into_element" => format!(
                "Type '{}' into {}",
                args.get("text_to_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                args.get("selector")
                    .and_then(|v| v.as_str())
                    .unwrap_or("field")
            ),
            "navigate_browser" => format!(
                "Navigate to {}",
                args.get("url").and_then(|v| v.as_str()).unwrap_or("URL")
            ),
            "select_option" => format!(
                "Select '{}' from dropdown",
                args.get("option_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("option")
            ),
            _ => format!("Execute {}", tool_name),
        }
    }

    fn get_wait_condition(&self, tool_name: &str) -> Option<String> {
        match tool_name {
            "click_element" => Some("Element state changes or UI updates".to_string()),
            "type_into_element" => Some("Text appears in field".to_string()),
            "navigate_browser" => Some("Page loads completely".to_string()),
            "open_application" => Some("Application window appears".to_string()),
            _ => None,
        }
    }

    fn extract_required_tools(&self, tool_calls: &[crate::utils::ToolCall]) -> Vec<String> {
        tool_calls
            .iter()
            .map(|tc| tc.tool_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    fn infer_expected_outcomes(&self, tool_calls: &[crate::utils::ToolCall]) -> Vec<String> {
        let mut outcomes = Vec::new();

        for call in tool_calls {
            match call.tool_name.as_str() {
                "navigate_browser" => {
                    outcomes.push("Target webpage loaded successfully".to_string())
                }
                "type_into_element" => outcomes.push("Form fields populated with data".to_string()),
                "click_element"
                    if call
                        .arguments
                        .get("selector")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .contains("Submit") =>
                {
                    outcomes.push("Form submitted successfully".to_string())
                }
                "select_option" => outcomes.push("Option selected in dropdown".to_string()),
                _ => {}
            }
        }

        outcomes
    }

    // Helper to optionally attach UI tree to response
    fn maybe_attach_tree(&self, include_tree: bool, pid_opt: Option<u32>, result_json: &mut Value) {
        if !include_tree {
            return;
        }
        if let Some(pid) = pid_opt {
            if let Ok(tree) = self.desktop.get_window_tree(pid, None, None) {
                if let Ok(tree_val) = serde_json::to_value(tree) {
                    if let Some(obj) = result_json.as_object_mut() {
                        obj.insert("ui_tree".to_string(), tree_val);
                    }
                }
            }
        }
    }
}

// Helper functions for export_workflow_sequence
fn should_add_focus_check(tool_calls: &[crate::utils::ToolCall], current_index: usize) -> bool {
    // Add focus check if:
    // 1. It's the first UI interaction
    // 2. Previous action was navigation or opened a new window
    // 3. There was a significant gap (e.g., after get_window_tree or wait)

    if current_index == 0 {
        return true;
    }

    let prev_tool = &tool_calls[current_index - 1].tool_name;
    matches!(
        prev_tool.as_str(),
        "navigate_browser"
            | "open_application"
            | "close_element"
            | "get_window_tree"
            | "get_applications"
            | "activate_element"
    )
}

fn is_state_changing_action(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "click_element"
            | "type_into_element"
            | "select_option"
            | "set_toggled"
            | "set_selected"
            | "set_range_value"
            | "invoke_element"
            | "press_key"
            | "mouse_drag"
            | "scroll_element"
    )
}

fn should_capture_tree(tool_name: &str, index: usize, total_steps: usize) -> bool {
    // Capture tree at key points:
    // 1. After major navigation
    // 2. Before complex sequences
    // 3. At regular intervals (every 5 steps)
    // 4. Before the final action

    matches!(tool_name, "navigate_browser" | "open_application")
        || index % 5 == 0
        || index == total_steps - 1
}

#[tool_handler]
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
        .unwrap_or_else(|_| "Unknown".to_string());

    format!(
        "
You are an AI assistant designed to control a computer desktop. Your primary goal is to understand the user's request and translate it into a sequence of tool calls to automate GUI interactions.

**Golden Rules for Robust Automation**

1.  **CHECK FOCUS FIRST:** Before any `click`, `type`, or `press_key` action, you **MUST** verify the target application `is_focused` using `get_applications`. If it's not, you **MUST** call `activate_element` before proceeding. This is the #1 way to prevent sending commands to the wrong window.

2.  **AVOID STALE STATE & CONTEXT COLLAPSE:** After any action that changes the UI context (closing a dialog, getting an error, a click that loads new content), the UI may have changed dramatically. **You MUST call `get_window_tree` again to get the current, fresh state before proceeding.** Failure to do so will cause you to act on a 'ghost' UI and fail. Do not trust a 'success' status alone; verify the outcome.

3.  **WAIT AFTER NAVIGATION:** After actions like `click_element` on a link or `navigate_browser`, the UI needs time to load. You **MUST** explicitly wait. The best method is to use `wait_for_element` targeting a known element on the new page. Do not call `get_window_tree` immediately.

4.  **CHECK BEFORE YOU ACT (Especially Toggles):** Before clicking a checkbox, radio button, or any toggleable item, **ALWAYS** use `is_toggled` or `is_selected` to check its current state. Only click if it's not already in the desired state to avoid accidentally undoing the action.

5.  **HANDLE DISABLED ELEMENTS:** Before attempting to click a button or interact with an element, you **MUST** check if it is enabled. The `validate_element` and `get_window_tree` tools return an `enabled` property. If an element is disabled (e.g., a grayed-out 'Submit' button), do not try to click it. Instead, you must investigate the UI to figure out why it's disabled. Look for unchecked checkboxes, empty required fields, or other dependencies that must be satisfied first.

6.  **USE PRECISE SELECTORS (ID IS YOUR FRIEND):** A `role|name` selector is good, but often, an element **does not have a `name` attribute** even if it contains visible text (the text is often a child element). Check the `get_window_tree` output carefully. If an element has an empty or generic name, you **MUST use its numeric ID (`\"#12345\"`) for selection.** Do not guess or hallucinate a `name` from the visual text; use the ID. This is critical for clickable `Group` elements which often lack a name.

7.  **PREFER INVOKE OVER CLICK FOR BUTTONS:** When dealing with buttons, especially those that might not be in the viewport, **prefer `invoke_element` over `click_element`**. The `invoke_element` action is more reliable because it doesn't require the element to be scrolled into view. Use `click_element` only when you specifically need mouse interaction behavior (e.g., for links or UI elements that respond differently to clicks).

8.  **USE SET_SELECTED FOR RADIO BUTTONS AND CHECKBOXES:** For radio buttons and selectable items, **always use `set_selected` with `state: true`** instead of `click_element`. This ensures the element reaches the desired state regardless of its current state. For checkboxes and toggle switches, use `set_toggled` with the desired state.


**Tool Behavior & Metadata**

Pay close attention to the tool descriptions for hints on their behavior.

*   **Read-only tools** are safe to use for inspection and will not change the UI state (e.g., `validate_element`, `get_window_tree`).
*   Tools that **may change the UI** require more care. After using one, consider calling `get_window_tree` again to get the latest UI state.
*   Tools that **require focus** must only be used on the foreground application. Use `get_applications` to check focus and `activate_element` to bring an application to the front.

**Core Workflow: Discover, then Act with Precision**

Your most reliable strategy is to inspect the application's UI structure *before* trying to interact with it. Never guess selectors.

1.  **Discover Running Applications:** Use `get_applications` to see what's running. This gives you the `name`, `id`, and `pid` (Process ID) for each application.

2.  **Get the UI Tree:** This is the most important step. Once you have the `pid` of your target application, call `get_window_tree` with `include_tree: true`. This returns a complete, JSON-like structure of all UI elements in that application.

3.  **Construct Smart Selector Strategies:** 
    *   **Primary Strategy:** Use `role:Type|name:Name` when available, otherwise use the numeric ID (`\"#12345\"`).
    *   **Multi-Selector Fallbacks:** Provide alternatives that are tried in parallel:
        ```json
        {{
          \"selector\": \"role:Button|name:Submit\",
          \"alternative_selectors\": \"#12345\"
        }}
        ```
    *   **Avoid:** Generic selectors like `\"role:Button\"` alone - they're too ambiguous.

**Action Examples**

*   **Invoking a button (preferred over clicking):**
    ```json
    {{
        \"tool_name\": \"invoke_element\",
        \"args\": {{\"selector\": \"role:button|name:Login\"}}
    }}
    ```
*   **Selecting a radio button (use set_selected, not click):**
    ```json
    {{
        \"tool_name\": \"set_selected\",
        \"args\": {{\"selector\": \"role:RadioButton|name:Male\", \"state\": true}}
    }}
    ```
*   **Typing an email into an email field:**
    ```json
    {{
        \"tool_name\": \"type_into_element\",
        \"args\": {{\"selector\": \"edit|Email\", \"text_to_type\": \"user@example.com\"}}
    }}
    ```
*   **Using alternative selectors for robustness:**
    ```json
    {{
        \"tool_name\": \"invoke_element\",
        \"args\": {{
            \"selector\": \"#17517999067772859239\",
            \"alternative_selectors\": \"role:Group|name:Run Quote\"
        }}
    }}
    ```

**Common Pitfalls & Solutions**

*   **Click fails on buttons not in viewport:** Use `invoke_element` instead of `click_element`.
*   **Radio button clicks don't register:** Use `set_selected` with `state: true`.
*   **Form validation errors:** Verify all fields AND radio buttons/checkboxes before submitting.
*   **Element not found after UI change:** Call `get_window_tree` again after UI changes.
*   **Selector matches wrong element:** Use numeric ID when name is empty.

Contextual information:
- The current date and time is {}.
- Current operating system: {}.
- Current working directory: {}.
",
        current_date_time, current_os, current_working_dir
    )
}
