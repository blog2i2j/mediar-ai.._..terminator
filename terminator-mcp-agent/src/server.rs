pub use crate::utils::DesktopWrapper;
use crate::utils::{
    get_timeout, ActivateElementArgs, ClickElementArgs, ClipboardArgs, EmptyArgs,
    GetApplicationsArgs, GetClipboardArgs, GetWindowTreeArgs, GetWindowsArgs, GlobalKeyArgs,
    HighlightElementArgs, LocatorArgs, MouseDragArgs, NavigateBrowserArgs, OpenApplicationArgs,
    PressKeyArgs, RunCommandArgs, ScrollElementArgs, SelectOptionArgs, SetRangeValueArgs,
    SetSelectedArgs, SetToggledArgs, TypeIntoElementArgs, ValidateElementArgs, WaitForElementArgs,
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
use terminator::{Browser, Desktop, Selector, UIElement};

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
    async fn get_window_tree(
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
        description = "Get all applications currently running and their state. This is a read-only operation."
    )]
    async fn get_applications(
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
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        // Clear the element before typing if requested (default: true)
        let should_clear = args.clear_before_typing.unwrap_or(true);
        if should_clear {
            // Select all existing text and delete it
            if let Err(clear_error) = element
                .press_key("{Ctrl}a")
                .and_then(|_| element.press_key("{Delete}"))
            {
                // If clearing fails, log it but continue with typing (non-fatal)
                eprintln!(
                    "Warning: Failed to clear element before typing: {}",
                    clear_error
                );
            }
        }

        element.type_text(&args.text_to_type, true).map_err(|e| {
            McpError::resource_not_found(
                "Failed to type text",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "text_to_type": args.text_to_type,
                    "element_info": element_info
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "type_into_element",
            "status": "success",
            "text_typed": args.text_to_type,
            "cleared_before_typing": should_clear,
            "element": element_info,
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

        // Always attach tree for better context, or if explicitly requested
        self.maybe_attach_tree(
            true,
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
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.click().map_err(|e| {
            McpError::resource_not_found(
                "Failed to click on element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "element_info": element_info
                })),
            )
        })?;

        // Build base result
        let mut result_json = json!({
            "action": "click",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        // --- Action Consequence Verification ---
        let mut consequence = "no_significant_change".to_string();
        std::thread::sleep(std::time::Duration::from_millis(250)); // Wait for UI to react

        // Check 1: Did the element disappear?
        let post_click_locator = self
            .desktop
            .locator(Selector::from(successful_selector.as_str()));
        if post_click_locator
            .wait(Some(std::time::Duration::from_millis(100)))
            .await
            .is_err()
        {
            consequence = "element_disappeared".to_string();
        } else {
            // Check 2: Did focus change?
            if let Ok(focused_element) = self.desktop.focused_element() {
                if focused_element.id_or_empty() != element.id_or_empty() {
                    consequence = format!("focus_changed_to: #{}", focused_element.id_or_empty());
                }
            }
        }

        if let Some(obj) = result_json.as_object_mut() {
            obj.insert("consequence".to_string(), json!(consequence));
        }
        // --- End Consequence Verification ---

        // Always attach tree for better context
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sends a key press to a UI element. Use curly brace format: '{Ctrl}c', '{Alt}{F4}', '{Enter}', '{PageDown}', etc. This action requires the application to be focused and may change the UI."
    )]
    async fn press_key(
        &self,
        Parameters(args): Parameters<PressKeyArgs>,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;
        let element_info = build_element_info(&element);

        element.press_key(&args.key).map_err(|e| {
            McpError::resource_not_found(
                "Failed to press key",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "key_pressed": args.key,
                    "element_info": element_info
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "press_key",
            "status": "success",
            "key_pressed": args.key,
            "element": element_info,
            "selector": args.selector,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

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
        let focused = self.desktop.focused_element().map_err(|e| {
            McpError::internal_error(
                "Failed to get focused element",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        // Gather metadata for debugging / result payload
        let element_info = build_element_info(&focused);

        // Perform the key press
        focused.press_key(&args.key).map_err(|e| {
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
        self.maybe_attach_tree(true, focused.process_id().ok(), &mut result_json);

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
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;

        let element_info = build_element_info(&element);

        element.activate_window().map_err(|e| {
            McpError::resource_not_found(
                "Failed to activate window with that element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "element_info": element_info
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "activate_element",
            "status": "success",
            "element": element_info,
            "selector": args.selector,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "recommendation": "Window activated successfully. The UI tree is attached to help you find specific elements to interact with next."
        });

        // Always attach UI tree for activated elements to help with next actions
        self.maybe_attach_tree(
            true, // Always attach tree for activate_element since it's important for next actions
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
        description = "Performs a mouse drag operation from start to end coordinates. This action requires the application to be focused and may change the UI."
    )]
    async fn mouse_drag(
        &self,
        Parameters(args): Parameters<MouseDragArgs>,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;

        // Get element details before dragging for better feedback
        let element_info = build_element_info(&element);

        element
            .mouse_drag(args.start_x, args.start_y, args.end_x, args.end_y)
            .map_err(|e| {
                McpError::resource_not_found(
                    "Failed to perform mouse drag",
                    Some(json!({
                        "reason": e.to_string(),
                        "selector": args.selector,
                        "start": (args.start_x, args.start_y),
                        "end": (args.end_x, args.end_y),
                        "element_info": element_info
                    })),
                )
            })?;

        let mut result_json = json!({
            "action": "mouse_drag",
            "status": "success",
            "element": element_info,
            "selector": args.selector,
            "start": (args.start_x, args.start_y),
            "end": (args.end_x, args.end_y),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Validates that an element exists and provides detailed information about it. This is a read-only operation."
    )]
    async fn validate_element(
        &self,
        Parameters(args): Parameters<ValidateElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        match find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        {
            Ok((element, successful_selector)) => {
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
                self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

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
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element for highlighting",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;

        let duration = args.duration_ms.map(std::time::Duration::from_millis);
        element.highlight(args.color, duration).map_err(|e| {
            McpError::internal_error(
                "Failed to highlight element",
                Some(json!({"reason": e.to_string(), "selector": args.selector})),
            )
        })?;

        let element_info = build_element_info(&element);

        let mut result_json = json!({
            "action": "highlight_element",
            "status": "success",
            "element": element_info,
            "selector": args.selector,
            "color": args.color.unwrap_or(0x0000FF),
            "duration_ms": args.duration_ms.unwrap_or(1000),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

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

        let condition_lower = args.condition.to_lowercase();
        let result = match condition_lower.as_str() {
            "exists" => match locator.wait(timeout).await {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            },
            "visible" => match locator.wait(timeout).await {
                Ok(element) => element.is_visible().map_err(|e| {
                    McpError::internal_error(
                        "Failed to check visibility",
                        Some(json!({"reason": e.to_string()})),
                    )
                }),
                Err(e) => Err(McpError::internal_error(
                    "Element not found",
                    Some(json!({"reason": e.to_string()})),
                )),
            },
            "enabled" => match locator.wait(timeout).await {
                Ok(element) => element.is_enabled().map_err(|e| {
                    McpError::internal_error(
                        "Failed to check enabled state",
                        Some(json!({"reason": e.to_string()})),
                    )
                }),
                Err(e) => Err(McpError::internal_error(
                    "Element not found",
                    Some(json!({"reason": e.to_string()})),
                )),
            },
            "focused" => match locator.wait(timeout).await {
                Ok(element) => element.is_focused().map_err(|e| {
                    McpError::internal_error(
                        "Failed to check focus state",
                        Some(json!({"reason": e.to_string()})),
                    )
                }),
                Err(e) => Err(McpError::internal_error(
                    "Element not found",
                    Some(json!({"reason": e.to_string()})),
                )),
            },
            _ => Err(McpError::invalid_params(
                "Invalid condition. Valid conditions: exists, visible, enabled, focused",
                Some(json!({"provided_condition": args.condition})),
            )),
        };

        match result {
            Ok(condition_met) => Ok(CallToolResult::success(vec![Content::json(json!({
                "action": "wait_for_element",
                "status": "success",
                "condition": args.condition,
                "condition_met": condition_met,
                "selector": args.selector,
                "timeout_ms": args.timeout_ms.unwrap_or(5000),
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))?])),
            Err(e) => Err(e),
        }
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
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element for closing",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;

        // Get element details before closing for better feedback
        let element_info = build_element_info(&element);

        element.close().map_err(|e| {
            McpError::resource_not_found(
                "Failed to close element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "element_info": element_info
                })),
            )
        })?;

        Ok(CallToolResult::success(vec![Content::json(json!({
            "action": "close_element",
            "status": "success",
            "element": element_info,
            "selector": args.selector,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))?]))
    }

    #[tool(description = "Scrolls a UI element in the specified direction by the given amount.")]
    async fn scroll_element(
        &self,
        Parameters(args): Parameters<ScrollElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        let locator = self.desktop.locator(Selector::from(args.selector.as_str()));
        let element = locator
            .wait(get_timeout(args.timeout_ms))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    "Failed to locate element for scrolling",
                    Some(json!({"reason": e.to_string(), "selector": args.selector})),
                )
            })?;

        // Get element details before scrolling for better feedback
        let element_info = build_element_info(&element);

        element.scroll(&args.direction, args.amount).map_err(|e| {
            McpError::resource_not_found(
                "Failed to scroll element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "direction": args.direction,
                    "amount": args.amount,
                    "element_info": element_info
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "scroll_element",
            "status": "success",
            "element": element_info,
            "selector": args.selector,
            "direction": args.direction,
            "amount": args.amount,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(description = "Selects an option in a dropdown or combobox by its visible text.")]
    async fn select_option(
        &self,
        Parameters(args): Parameters<SelectOptionArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.select_option(&args.option_name).map_err(|e| {
            McpError::resource_not_found(
                "Failed to select option",
                Some(
                    json!({ "reason": e.to_string(), "selector": args.selector, "option": args.option_name }),
                ),
            )
        })?;

        let mut result_json = json!({
            "action": "select_option",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "option_selected": args.option_name,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Lists all available option strings from a dropdown, list box, or similar control. This is a read-only operation."
    )]
    async fn list_options(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);
        let options = element.list_options().map_err(|e| {
            McpError::internal_error(
                "Failed to list options",
                Some(json!({ "reason": e.to_string(), "selector": args.selector })),
            )
        })?;

        let mut result_json = json!({
            "action": "list_options",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "options": options,
            "count": options.len(),
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the state of a toggleable control (e.g., checkbox, switch). This action requires the application to be focused and may change the UI."
    )]
    async fn set_toggled(
        &self,
        Parameters(args): Parameters<SetToggledArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.set_toggled(args.state).map_err(|e| {
            McpError::internal_error(
                "Failed to set toggle state",
                Some(
                    json!({ "reason": e.to_string(), "selector": args.selector, "state": args.state }),
                ),
            )
        })?;

        let mut result_json = json!({
            "action": "set_toggled",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "state_set_to": args.state,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the value of a range-based control like a slider. This action requires the application to be focused and may change the UI."
    )]
    async fn set_range_value(
        &self,
        Parameters(args): Parameters<SetRangeValueArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.set_range_value(args.value).map_err(|e| {
            McpError::internal_error(
                "Failed to set range value",
                Some(
                    json!({ "reason": e.to_string(), "selector": args.selector, "value": args.value }),
                ),
            )
        })?;

        let mut result_json = json!({
            "action": "set_range_value",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "value_set_to": args.value,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Sets the selection state of a selectable item (e.g., in a list or calendar). This action requires the application to be focused and may change the UI."
    )]
    async fn set_selected(
        &self,
        Parameters(args): Parameters<SetSelectedArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.set_selected(args.state).map_err(|e| {
            McpError::internal_error(
                "Failed to set selected state",
                Some(
                    json!({ "reason": e.to_string(), "selector": args.selector, "state": args.state }),
                ),
            )
        })?;

        let mut result_json = json!({
            "action": "set_selected",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "state_set_to": args.state,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Checks if a control (like a checkbox or toggle switch) is currently toggled on. This is a read-only operation."
    )]
    async fn is_toggled(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);
        let is_toggled = element.is_toggled().map_err(|e| {
            McpError::internal_error(
                "Failed to get toggle state",
                Some(json!({ "reason": e.to_string(), "selector": args.selector })),
            )
        })?;

        let mut result_json = json!({
            "action": "is_toggled",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "is_toggled": is_toggled,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Gets the current value from a range-based control like a slider or progress bar. This is a read-only operation."
    )]
    async fn get_range_value(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);
        let value = element.get_range_value().map_err(|e| {
            McpError::internal_error(
                "Failed to get range value",
                Some(json!({ "reason": e.to_string(), "selector": args.selector })),
            )
        })?;

        let mut result_json = json!({
            "action": "get_range_value",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "value": value,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(
        description = "Checks if a selectable item (e.g., in a calendar, list, or tab) is currently selected. This is a read-only operation."
    )]
    async fn is_selected(
        &self,
        Parameters(args): Parameters<LocatorArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);
        let is_selected = element.is_selected().map_err(|e| {
            McpError::internal_error(
                "Failed to get selected state",
                Some(json!({ "reason": e.to_string(), "selector": args.selector })),
            )
        })?;

        let mut result_json = json!({
            "action": "is_selected",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "is_selected": is_selected,
        });
        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);
        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
    }

    #[tool(description = "Captures a screenshot of a specific UI element.")]
    async fn capture_element_screenshot(
        &self,
        Parameters(args): Parameters<ValidateElementArgs>,
    ) -> Result<CallToolResult, McpError> {
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            let selectors_tried =
                get_selectors_tried(&args.selector, args.alternative_selectors.as_deref());
            McpError::internal_error(
                "Failed to locate element for screenshot",
                Some(json!({
                    "reason": e.to_string(),
                    "selectors_tried": selectors_tried,
                })),
            )
        })?;

        let screenshot_result = element.capture().map_err(|e| {
            McpError::internal_error(
                "Failed to capture element screenshot",
                Some(json!({"reason": e.to_string(), "selector_used": successful_selector})),
            )
        })?;

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
        use crate::utils::find_element_with_fallbacks;

        let (element, successful_selector) = find_element_with_fallbacks(
            &self.desktop,
            &args.selector,
            args.alternative_selectors.as_deref(),
            args.timeout_ms,
        )
        .await
        .map_err(|e| {
            build_element_not_found_error(
                &args.selector,
                args.alternative_selectors.as_deref(),
                e.into(),
            )
        })?;

        let element_info = build_element_info(&element);

        element.invoke().map_err(|e| {
            McpError::resource_not_found(
                "Failed to invoke element",
                Some(json!({
                    "reason": e.to_string(),
                    "selector": args.selector,
                    "element_info": element_info.clone()
                })),
            )
        })?;

        let mut result_json = json!({
            "action": "invoke",
            "status": "success",
            "element": element_info,
            "selector_used": successful_selector,
            "selectors_tried": get_selectors_tried(&args.selector, args.alternative_selectors.as_deref()),
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        self.maybe_attach_tree(true, element.process_id().ok(), &mut result_json);

        Ok(CallToolResult::success(vec![Content::json(result_json)?]))
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

**Tool Behavior & Metadata**

Pay close attention to the tool descriptions for hints on their behavior.

*   **Read-only tools** are safe to use for inspection and will not change the UI state (e.g., `validate_element`, `get_window_tree`).
*   Tools that **may change the UI** require more care. After using one, consider calling `get_window_tree` again to get the latest UI state.
*   Tools that **require focus** must only be used on the foreground application. Use `get_applications` to check focus and `activate_element` to bring an application to the front.

**Core Workflow: Discover, then Act with Precision**

Your most reliable strategy is to inspect the application's UI structure *before* trying to interact with it. Never guess selectors.

1.  **Discover Running Applications:** Use `get_applications` to see what's running. This gives you the `name`, `id`, and `pid` (Process ID) for each application.

2.  **Get the UI Tree:** This is the most important step. Once you have the `pid` of your target application, call `get_window_tree` with `include_tree: true`. This returns a complete, JSON-like structure of all UI elements in that application.

3.  **Construct Smart Selector Strategies:** You have powerful tools to create robust targeting strategies:
    *   **Primary Strategy - Role+Name, then ID:** Always check the `get_window_tree` output. If an element has a unique, non-generic `name` attribute, use `role|name`. Otherwise, **immediately use the numeric ID selector (`\"#12345\"`)**. An element's visible text is often in a child element, so the parent container (like a `Group`) may not have a `name` itself.
    *   **Multi-Selector Fallbacks:** Provide precise alternatives, which are tried in parallel.
        ```json
        {{
          \"selector\": \"button|Submit Form\",
          \"alternative_selectors\": \"#12345,role:Document >> button|Submit\"
        }}
        ```
    *   **Chain Selectors for Context:** Use `>>` to chain selectors for specificity. Examples:
        - `\"role:Document >> name:Email\"` (find Email field within document content)
        - `\"role:Window >> role:Document >> role:Button\"` (find button within document, not browser chrome)
        - `\"name:Form >> role:Edit\"` (find edit field within a specific form)
    *   **Coordinate-Based Selectors for Visual Targeting:** When dealing with graphical elements that lack stable IDs or names (like canvas elements, drawing applications, or specific parts of an image), you can use coordinate-based selectors:
        - `pos:x,y`: To target a specific point on the screen for actions like `click_element`.
    *   **Intelligent Selector Design:** Consider the application context:
        - **Web browsers:** Chain with `role:Document >>` to target page content, not browser UI
        - **Forms:** Use parent containers to avoid ambiguity: `\"name:Contact Form >> name:Email\"`
        - **Complex apps:** Navigate from window → document → specific element
    *   **Smart Fallback Strategy:** Order selectors from most to least specific:
        1. Role+Name (pipe): `\"button|Submit\"`
        2. Exact numeric ID: `\"#12345\"`
        3. Contextual chain: `\"role:Document >> button|Submit\"`
    *   Note: AVOID: Generic selectors like `\"role:Button\"` or `\"name:Submit\"` alone.

4.  **Interact with the Element:** Once you have a reliable `selector`, use an action tool:
    *   `invoke_element`: **(Preferred Method)** Use this for most interactions. It's faster and more reliable than `click`, especially for controls like radio buttons, menu items, and checkboxes. It triggers the element's default action directly.
    *   `click_element`: Use as a fallback if `invoke_element` fails or for elements where a literal mouse click is necessary (e.g., specific coordinates on a canvas).
    *   `type_into_element`: To type text into input fields.
    *   `press_key`: Sends a key press to a UI element. **Key Syntax: Use curly braces for special keys!**
    *   `activate_element`: To bring a window to the foreground.
    *   `mouse_drag`: To perform drag and drop operations.
    *   `set_clipboard`: To set text to the system clipboard.
    *   `get_clipboard`: To get text from the system clipboard.
    *   `scroll_element`: To scroll within elements like web pages, documents, or lists.

**Action Examples**

*   **Clicking a 'Login' button:**
    ```json
    {{
        \"tool_name\": \"invoke_element\",
        \"args\": {{\"selector\": \"button|Login\"}}
    }}
    ```
*   **Typing an email into an email field:**
    ```json
    {{
        \"tool_name\": \"type_into_element\",
        \"args\": {{\"selector\": \"edit|Email\", \"text_to_type\": \"user@example.com\"}}
    }}
    ```
*   **Typing height into a height field:**
    ```json
    {{
        \"tool_name\": \"type_into_element\",
        \"args\": {{\"selector\": \"edit|Height\", \"text_to_type\": \"6'2\"}}
    }}
    ```

5.  **Handle Scrolling for Full Context:** When working with pages or long content, ALWAYS scroll to see all content. Use `scroll_element` to scroll pages up/down to get the full context before making decisions or extracting information.

Contextual information:
- The current date and time is {}.
- Current operating system: {}.
- Current working directory: {}.
",
        current_date_time, current_os, current_working_dir
    )
}
