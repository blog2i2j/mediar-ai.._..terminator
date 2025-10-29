use crate::events::{
    BrowserClickEvent, BrowserTabNavigationEvent, ButtonInteractionType, ClickEvent, TabAction,
    TabNavigationMethod,
};
use crate::recorder::browser_context::BrowserContextRecorder;
use crate::{
    ApplicationSwitchMethod, ClipboardAction, ClipboardEvent, EventMetadata, HotkeyEvent,
    KeyboardEvent, MouseButton, MouseEvent, MouseEventType, Position, Result, WorkflowEvent,
    WorkflowRecorderConfig,
};
use arboard::Clipboard;
use rdev::{Button, EventType};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};
use sysinfo::{Pid, ProcessesToUpdate, System};
use terminator::{convert_uiautomation_element_to_terminator, UIElement};

use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uiautomation::types::Point;
use uiautomation::UIAutomation;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_QUIT,
};

pub mod structs;
use structs::*;

// Bundle related parameters for UIA button press handling to keep function signatures small
struct ButtonPressContext<'a> {
    position: &'a Position,
    config: &'a WorkflowRecorderConfig,
    current_text_input: &'a Arc<Mutex<Option<TextInputTracker>>>,
    event_tx: &'a broadcast::Sender<WorkflowEvent>,
    performance_last_event_time: &'a Arc<Mutex<Instant>>,
    performance_events_counter: &'a Arc<Mutex<(u32, Instant)>>,
    is_stopping: &'a Arc<AtomicBool>,
    double_click_tracker: &'a Arc<Mutex<structs::DoubleClickTracker>>,
    browser_recorder: &'a Arc<Mutex<Option<BrowserContextRecorder>>>,
    tokio_runtime: &'a Arc<Mutex<Option<Runtime>>>,
}

/// The Windows-specific recorder
pub struct WindowsRecorder {
    /// The event sender
    event_tx: broadcast::Sender<WorkflowEvent>,

    /// The configuration
    config: WorkflowRecorderConfig,

    /// The last mouse position
    last_mouse_pos: Arc<Mutex<Option<(i32, i32)>>>,

    /// Signal to stop the listener thread
    stop_indicator: Arc<AtomicBool>,

    /// Signal that we're in the stopping phase (prevents new events from being added)
    is_stopping: Arc<AtomicBool>,

    /// Modifier key states
    modifier_states: Arc<Mutex<ModifierStates>>,

    /// Last clipboard content hash for change detection
    last_clipboard_hash: Arc<Mutex<Option<u64>>>,

    /// Last mouse move time for throttling
    last_mouse_move_time: Arc<Mutex<Instant>>,

    /// Known hotkey patterns
    hotkey_patterns: Arc<Vec<HotkeyPattern>>,

    /// UI Automation thread ID for proper cleanup
    ui_automation_thread_id: Arc<Mutex<Option<u32>>>,

    /// Current application tracking for switch detection
    current_application: Arc<Mutex<Option<ApplicationState>>>,

    /// Browser tab navigation tracking
    browser_tab_tracker: Arc<Mutex<BrowserTabTracker>>,

    /// Alt+Tab tracking for application switch attribution
    alt_tab_tracker: Arc<Mutex<AltTabTracker>>,

    /// Rate limiting for performance modes
    last_event_time: Arc<Mutex<std::time::Instant>>,

    /// Event counter for rate limiting
    events_this_second: Arc<Mutex<(u32, std::time::Instant)>>,

    /// Currently focused text input element tracking with keystroke counting
    current_text_input: Arc<Mutex<Option<TextInputTracker>>>,

    /// Double click detection tracker
    double_click_tracker: Arc<Mutex<structs::DoubleClickTracker>>,

    /// Browser context recorder for DOM capture
    browser_recorder: Arc<Mutex<Option<BrowserContextRecorder>>>,

    /// Tokio runtime for async browser operations
    tokio_runtime: Arc<Mutex<Option<Runtime>>>,
}

impl WindowsRecorder {
    /// Capture the current timestamp in milliseconds since epoch
    fn capture_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    /// Collect text content from only direct children (no recursion for deepest element approach)
    fn collect_direct_child_text_content(element: &UIElement) -> Vec<String> {
        let mut child_texts = Vec::new();

        // Get only direct children of this element
        if let Ok(children) = element.children() {
            for child in children {
                // Collect the child's own text/name
                if let Some(child_name) = child.name() {
                    if !child_name.trim().is_empty() {
                        child_texts.push(child_name.trim().to_string());
                    }
                }
                // No recursion - we're already at the deepest element
            }
        }

        // Remove duplicates and empty strings
        child_texts.sort();
        child_texts.dedup();
        child_texts
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Recursively collect text content from all child elements (unlimited depth) - legacy method
    fn collect_child_text_content(element: &UIElement) -> Vec<String> {
        let mut child_texts = Vec::new();

        // Get all children of this element
        if let Ok(children) = element.children() {
            for child in children {
                // Collect the child's own text/name
                if let Some(child_name) = child.name() {
                    if !child_name.trim().is_empty() {
                        child_texts.push(child_name.trim().to_string());
                    }
                }

                // Recursively collect from deeper levels (unlimited depth)
                let deeper_texts = Self::collect_child_text_content(&child);
                child_texts.extend(deeper_texts);
            }
        }

        // Remove duplicates and empty strings
        child_texts.sort();
        child_texts.dedup();
        child_texts
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Creates a UIAutomation instance with the configured threading model for a new thread.
    fn create_configured_automation_instance(
        config: &WorkflowRecorderConfig,
    ) -> std::result::Result<UIAutomation, String> {
        unsafe {
            let threading_model = if config.enable_multithreading {
                COINIT_MULTITHREADED
            } else {
                COINIT_APARTMENTTHREADED
            };
            let hr = CoInitializeEx(None, threading_model);
            if hr.is_err() && hr != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                let err_msg = format!("Failed to initialize COM for new thread: {hr:?}");
                error!("{}", err_msg);
                return Err(err_msg);
            }
        }
        UIAutomation::new_direct().map_err(|e| {
            let err_msg = format!("Failed to create UIAutomation instance directly: {e}");
            error!("{}", err_msg);
            err_msg
        })
    }

    /// Create a new Windows recorder
    pub async fn new(
        config: WorkflowRecorderConfig,
        event_tx: broadcast::Sender<WorkflowEvent>,
    ) -> Result<Self> {
        info!("Initializing comprehensive Windows recorder");
        debug!("Recorder config: {:?}", config);

        let last_mouse_pos = Arc::new(Mutex::new(None));
        let stop_indicator = Arc::new(AtomicBool::new(false));
        let modifier_states = Arc::new(Mutex::new(ModifierStates {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
        }));
        let last_clipboard_hash = Arc::new(Mutex::new(None));
        let last_mouse_move_time = Arc::new(Mutex::new(Instant::now()));

        // Initialize hotkey patterns
        let hotkey_patterns = Arc::new(Self::initialize_hotkey_patterns());

        // Initialize browser recorder and tokio runtime
        let browser_recorder = BrowserContextRecorder::new();
        let tokio_runtime = Runtime::new().ok();

        // Check if Chrome extension is available
        if let Some(ref runtime) = tokio_runtime {
            let recorder_clone = browser_recorder.clone();
            let _available = runtime.spawn(async move {
                if recorder_clone.is_extension_available().await {
                    info!("✅ Chrome extension is available for browser recording");
                } else {
                    warn!("⚠️ Chrome extension not available - browser recording disabled");
                }
            });
        }

        let mut recorder = Self {
            event_tx,
            config,
            last_mouse_pos,
            stop_indicator,
            is_stopping: Arc::new(AtomicBool::new(false)),
            modifier_states,
            last_clipboard_hash,
            last_mouse_move_time,
            hotkey_patterns,
            ui_automation_thread_id: Arc::new(Mutex::new(None)),
            current_application: Arc::new(Mutex::new(None)),
            browser_tab_tracker: Arc::new(Mutex::new(BrowserTabTracker::default())),
            alt_tab_tracker: Arc::new(Mutex::new(AltTabTracker::default())),
            last_event_time: Arc::new(Mutex::new(Instant::now())),
            events_this_second: Arc::new(Mutex::new((0, Instant::now()))),
            current_text_input: Arc::new(Mutex::new(None)),
            double_click_tracker: Arc::new(Mutex::new(structs::DoubleClickTracker::default())),
            browser_recorder: Arc::new(Mutex::new(Some(browser_recorder))),
            tokio_runtime: Arc::new(Mutex::new(tokio_runtime)),
        };

        let handle = tokio::runtime::Handle::current();

        // Set up comprehensive event listeners
        recorder.setup_comprehensive_listeners(handle).await?;

        Ok(recorder)
    }

    /// Check for application switch and emit event if detected
    fn check_and_emit_application_switch(
        current_app: &Arc<Mutex<Option<ApplicationState>>>,
        event_tx: &broadcast::Sender<WorkflowEvent>,
        new_element: &Option<UIElement>,
        default_switch_method: ApplicationSwitchMethod,
        config: &WorkflowRecorderConfig,
        alt_tab_tracker: &Arc<Mutex<AltTabTracker>>,
        browser_tab_tracker: &Arc<Mutex<BrowserTabTracker>>,
    ) {
        warn!(
            "🔍 CHECKING APPLICATION SWITCH - called with element: {:?}",
            new_element.as_ref().map(|e| e.application_name())
        );

        if !config.record_application_switches {
            return;
        }

        if let Some(element) = new_element {
            let app_name = element.application_name();
            if let Ok(process_id) = element.process_id() {
                if !app_name.is_empty() {
                    let mut current = current_app.lock().unwrap();

                    // Check if this is a new application
                    let is_switch = if let Some(ref current_state) = *current {
                        current_state.process_id != process_id || current_state.name != app_name
                    } else {
                        true // First app detection
                    };

                    if is_switch {
                        let now = Instant::now();

                        // Determine the actual switch method - check for Alt+Tab first
                        let actual_switch_method =
                            if let Ok(mut tracker) = alt_tab_tracker.try_lock() {
                                if tracker.consume_pending_alt_tab() {
                                    ApplicationSwitchMethod::AltTab
                                } else {
                                    default_switch_method
                                }
                            } else {
                                default_switch_method
                            };

                        // Calculate dwell time for previous app
                        let dwell_time = if let Some(ref current_state) = *current {
                            let duration = now.duration_since(current_state.start_time);
                            if duration.as_millis()
                                >= config.app_switch_dwell_time_threshold_ms as u128
                            {
                                Some(duration.as_millis() as u64)
                            } else {
                                None // Too short, probably just UI noise
                            }
                        } else {
                            None
                        };

                        // Get process name for current and target process (do this BEFORE any conditional blocks)
                        let mut system = System::new();
                        system.refresh_processes(ProcessesToUpdate::All, true);

                        let from_process_name = current.as_ref().and_then(|s| {
                            system
                                .process(Pid::from_u32(s.process_id))
                                .map(|p| p.name().to_string_lossy().to_string())
                        });

                        let to_process_name = system
                            .process(Pid::from_u32(process_id))
                            .map(|p| p.name().to_string_lossy().to_string());

                        // Only emit if we have meaningful dwell time or this is first app
                        if dwell_time.is_some() || current.is_none() {
                            // Check if this is a browser and try to get URL
                            let is_browser = app_name.to_lowercase().contains("chrome")
                                || app_name.to_lowercase().contains("firefox")
                                || app_name.to_lowercase().contains("edge");

                            if is_browser {
                                warn!("🌐 Browser detected: {}, element role: {}, attempting URL detection", 
                                     app_name, element.role());
                                // Try to find the browser URL before emitting event
                                if let Some(url) = Self::proactive_browser_url_search(element) {
                                    warn!("✅ Successfully found browser URL: {}", url);
                                    // Emit browser navigation event instead of application switch
                                    let nav_event = crate::BrowserTabNavigationEvent {
                                        action: crate::events::TabAction::Switched,
                                        method: crate::events::TabNavigationMethod::Other,
                                        to_url: Some(url.clone()),
                                        from_url: None,
                                        to_title: Some(app_name.clone()),
                                        from_title: current.as_ref().map(|s| s.name.clone()),
                                        browser: app_name.clone(),
                                        tab_index: None,
                                        total_tabs: None,
                                        page_dwell_time_ms: dwell_time,
                                        is_back_forward: false,
                                        metadata: EventMetadata::with_ui_element_and_timestamp(
                                            Some(element.clone()),
                                        ),
                                    };

                                    if let Err(e) = event_tx
                                        .send(WorkflowEvent::BrowserTabNavigation(nav_event))
                                    {
                                        debug!("Failed to send browser navigation event: {}", e);
                                    } else {
                                        info!(
                                            "🌐 Browser navigation event sent: {} → {} ({})",
                                            current
                                                .as_ref()
                                                .map(|s| s.name.as_str())
                                                .unwrap_or("(none)"),
                                            app_name,
                                            url
                                        );
                                    }

                                    // Also update browser tab tracker
                                    if let Ok(mut tracker) = browser_tab_tracker.try_lock() {
                                        tracker.current_url = Some(url.clone());
                                        tracker.current_browser = Some(app_name.clone());
                                        tracker.current_title = Some(app_name.clone());
                                        tracker.last_navigation_time = now;
                                    }

                                    // Update current app state and return early
                                    *current = Some(ApplicationState {
                                        name: app_name.clone(),
                                        process_name: to_process_name.clone(),
                                        process_id,
                                        start_time: now,
                                    });

                                    return; // Don't emit ApplicationSwitch event
                                } else {
                                    warn!("⚠️ Browser detected but no URL found");
                                }
                            }

                            // Not a browser or couldn't find URL - emit normal application switch
                            let event = crate::ApplicationSwitchEvent {
                                from_window_and_application_name: current
                                    .as_ref()
                                    .map(|s| s.name.clone()),
                                to_window_and_application_name: app_name.clone(),
                                from_process_name: from_process_name.clone(),
                                to_process_name: to_process_name.clone(),
                                from_process_id: current.as_ref().map(|s| s.process_id),
                                to_process_id: process_id,
                                switch_method: actual_switch_method.clone(),
                                dwell_time_ms: dwell_time,
                                switch_count: None,
                                metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                                    element.clone(),
                                )),
                            };

                            if let Err(e) = event_tx.send(WorkflowEvent::ApplicationSwitch(event)) {
                                debug!("Failed to send application switch event: {}", e);
                            } else {
                                debug!(
                                    "✅ Application switch event sent: {} -> {} (method: {:?})",
                                    current
                                        .as_ref()
                                        .map(|s| s.name.as_str())
                                        .unwrap_or("(none)"),
                                    app_name,
                                    actual_switch_method
                                );
                            }
                        }

                        // Update current application state
                        *current = Some(ApplicationState {
                            name: app_name.clone(),
                            process_name: to_process_name.clone(),
                            process_id,
                            start_time: now,
                        });

                        // Initialize browser tracker if switching to a browser
                        let app_name_lower = app_name.to_lowercase();
                        let is_browser = [
                            "chrome", "firefox", "edge", "brave", "opera", "safari", "vivaldi",
                        ]
                        .iter()
                        .any(|b| app_name_lower.contains(b));

                        if is_browser {
                            warn!(
                                "🌐 BROWSER DETECTED: {} - attempting proactive URL search",
                                app_name
                            );
                            // Proactively search for URL when switching to browser
                            if let Some(url) = Self::proactive_browser_url_search(element) {
                                let mut tracker = browser_tab_tracker.lock().unwrap();

                                // Always update URL when switching to browser to catch timestamp changes
                                let should_update = match &tracker.current_url {
                                    None => true, // No URL yet
                                    Some(current) => {
                                        // Update if URL changed (including query params like timestamps)
                                        current != &url
                                    }
                                };

                                if should_update {
                                    warn!(
                                        "Proactive URL detection on browser switch to {}: {} (was: {:?})",
                                        app_name, url, tracker.current_url
                                    );

                                    // Update tracker
                                    tracker.current_url = Some(url.clone());
                                    tracker.current_browser = Some(app_name.clone());
                                    tracker.current_title = Some(element.window_title());
                                    tracker.last_navigation_time = now;

                                    // Emit browser navigation event
                                    let nav_event = BrowserTabNavigationEvent {
                                        action: TabAction::Switched,
                                        method: TabNavigationMethod::TabClick,
                                        to_url: Some(url.clone()),
                                        from_url: None, // We don't track the previous URL on app switch
                                        to_title: Some(element.window_title()),
                                        from_title: None,
                                        browser: app_name.clone(),
                                        tab_index: None,
                                        total_tabs: None,
                                        page_dwell_time_ms: None,
                                        is_back_forward: false,
                                        metadata: EventMetadata::with_ui_element_and_timestamp(
                                            Some(element.clone()),
                                        ),
                                    };

                                    warn!("📍 Emitting browser navigation event for URL: {}", url);
                                    if let Err(e) = event_tx
                                        .send(WorkflowEvent::BrowserTabNavigation(nav_event))
                                    {
                                        debug!("Failed to send browser navigation event: {}", e);
                                    }
                                } else {
                                    debug!(
                                        "Browser switch to {} - URL unchanged: {}",
                                        app_name, url
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Proactively search for URL in browser window
    fn proactive_browser_url_search(element: &UIElement) -> Option<String> {
        warn!(
            "🔍 Starting proactive URL search from element: role={}, name={:?}",
            element.role(),
            element.name()
        );

        // Try direct URL property first (fast path)
        if let Some(url) = element.url() {
            if !url.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
                warn!("✅ Found valid URL directly on element: {}", url);
                return Some(url);
            } else if !url.is_empty() {
                warn!(
                    "⚠️ Found non-HTTP URL on element: {} - continuing search",
                    url
                );
            }
        }
        warn!("❌ No direct valid URL on element, starting deep search");

        // Deep recursive search for URL (handles deeply nested browser UIs)
        if let Some(url) = Self::deep_url_search(element, 0, 10) {
            return Some(url);
        }

        // If we still haven't found a URL, try to navigate up to the window root
        warn!("🔍 Attempting to find window root for comprehensive search");
        let mut current = element.clone();
        let mut depth = 0;
        const MAX_PARENT_DEPTH: usize = 10;

        // Navigate up the tree to find the window element
        while depth < MAX_PARENT_DEPTH {
            if let Ok(Some(parent)) = current.parent() {
                let parent_role = parent.role();
                warn!(
                    "📍 Parent at depth {}: role={}, name={:?}",
                    depth,
                    parent_role,
                    parent.name()
                );

                // If we found a Window element that looks like a browser, search from there
                if parent_role == "Window" {
                    let window_name = parent.name().unwrap_or_default();
                    let is_browser_window = window_name.to_lowercase().contains("chrome")
                        || window_name.to_lowercase().contains("firefox")
                        || window_name.to_lowercase().contains("edge")
                        || window_name.to_lowercase().contains("safari");

                    if is_browser_window {
                        warn!(
                            "🎯 Found browser Window element: {}, searching from window root",
                            window_name
                        );

                        // First try to find the address bar directly (more reliable for Chrome)
                        if let Some(url) = Self::find_address_bar_url(&parent, 0, 10) {
                            warn!("✅ Found URL in address bar");
                            return Some(url);
                        }

                        // Then try deep search for Documents
                        if let Some(url) = Self::deep_url_search(&parent, 0, 15) {
                            warn!("✅ Found URL by deep searching browser window");
                            return Some(url);
                        }
                        warn!("⚠️ No URL found in browser window, will continue searching up");
                    } else {
                        // Check if this is a modal dialog from a website
                        if window_name.contains(" says") || window_name.contains(" alert") {
                            // Extract domain from modal dialog title like "domain.com:8080 says"
                            if let Some(domain_part) = window_name.split(" says").next() {
                                let domain = domain_part.trim();
                                if domain.contains('.') && !domain.contains(' ') {
                                    // Build URL from domain
                                    let url = if domain.contains("://") {
                                        domain.to_string()
                                    } else if domain.starts_with("localhost")
                                        || domain.contains(":")
                                    {
                                        format!("http://{domain}")
                                    } else {
                                        format!("https://{domain}")
                                    };
                                    warn!("✅ Extracted URL from modal dialog title: {}", url);
                                    return Some(url);
                                }
                            }
                        }
                        warn!(
                            "⚠️ Found non-browser Window: {} - continuing up",
                            window_name
                        );
                    }
                }

                current = parent;
                depth += 1;
            } else {
                // No more parents - only search if we haven't reached Desktop level
                if depth > 0 {
                    let current_name = current.name().unwrap_or_default();
                    if current_name.contains("Desktop") {
                        warn!("⚠️ Reached Desktop level - stopping search to avoid traversing entire desktop");
                    } else {
                        warn!(
                            "🔍 Reached top of tree at depth {}, searching from highest parent",
                            depth
                        );
                        if let Some(url) = Self::deep_url_search(&current, 0, 10) {
                            warn!("✅ Found URL by searching from highest parent");
                            return Some(url);
                        }
                    }
                }
                break;
            }
        }

        // Try parsing from window title as last resort
        let window_title = element.window_title();
        if !window_title.is_empty() {
            // Common patterns: "Page Title - Domain - Browser"
            if let Some(url) = Self::extract_url_from_title(&window_title) {
                debug!("Extracted URL from window title: {}", url);
                return Some(url);
            }
        }

        warn!("❌ No URL found in any search method");
        None
    }

    /// Prioritized search for browser URL - address bar first, then main Document
    fn deep_url_search(element: &UIElement, depth: usize, max_depth: usize) -> Option<String> {
        if depth > max_depth {
            return None;
        }

        // First pass: Look for address bar (highest priority)
        if let Some(url) = Self::find_address_bar_url(element, depth, max_depth) {
            warn!("✅ Found URL in address bar: {}", url);
            return Some(url);
        }

        // Second pass: Look for main Document element (second priority)
        if let Some(url) = Self::find_main_document_url(element, depth, max_depth) {
            warn!("✅ Found URL in main Document: {}", url);
            return Some(url);
        }

        None
    }

    /// Find URL in address bar (Edit control)
    fn find_address_bar_url(element: &UIElement, depth: usize, max_depth: usize) -> Option<String> {
        if depth > max_depth {
            return None;
        }

        let role = element.role();

        // Check Edit controls (address bar)
        if role == "Edit" {
            if let Some(name) = element.name() {
                let name_lower = name.to_lowercase();
                // Check if this is likely an address bar
                if name_lower.contains("address")
                    || name_lower.contains("search bar")
                    || name_lower.contains("location")
                    || name_lower.contains("url")
                {
                    if let Ok(text) = element.text(0) {
                        // Check if it's a URL or domain
                        if text.starts_with("http://") || text.starts_with("https://") {
                            return Some(text);
                        } else if text.contains('.')
                            && !text.contains(' ')
                            && !text.starts_with("chrome-error://")
                            && !text.starts_with("about:")
                            && !text.starts_with("edge://")
                        {
                            // Domain without protocol (but not an error page)
                            return Some(format!("https://{text}"));
                        }
                    }
                }
            }
        }

        // Recursively search children
        if let Ok(children) = element.children() {
            for child in children {
                if let Some(url) = Self::find_address_bar_url(&child, depth + 1, max_depth) {
                    return Some(url);
                }
            }
        }

        None
    }

    /// Find URL in main Document element only (not nested iframes or links)
    fn find_main_document_url(
        element: &UIElement,
        depth: usize,
        max_depth: usize,
    ) -> Option<String> {
        if depth > max_depth {
            return None;
        }

        let role = element.role();

        // Check Document elements at shallow depths (main page, not nested content)
        if role == "Document" && depth <= 5 {
            // Main documents are usually at shallow depths
            // First try the text property which often contains the URL
            if let Ok(text) = element.text(0) {
                if text.starts_with("http://") || text.starts_with("https://") {
                    return Some(text);
                }
            }
            // Fallback to url() method
            if let Some(url) = element.url() {
                if !url.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) {
                    return Some(url);
                }
            }
        }

        // Recursively search children for main document
        if let Ok(children) = element.children() {
            for child in children {
                // Skip modal dialog windows
                let child_role = child.role();
                if child_role == "Window" {
                    let child_name = child.name().unwrap_or_default();
                    if child_name.contains("says")
                        || child_name.contains("alert")
                        || child_name.contains("dialog")
                        || child_name.contains("popup")
                    {
                        continue;
                    }
                }

                if let Some(url) = Self::find_main_document_url(&child, depth + 1, max_depth) {
                    return Some(url);
                }
            }
        }

        None
    }

    /// Helper to search an element and its children for URL
    #[allow(dead_code)]
    fn search_element_for_url(element: &UIElement) -> Option<String> {
        // Check direct URL
        if let Some(url) = element.url() {
            if !url.is_empty() {
                return Some(url);
            }
        }

        // Search children
        if let Ok(children) = element.children() {
            for child in children {
                let role = child.role();
                if role == "Document" {
                    if let Some(url) = child.url() {
                        if !url.is_empty() {
                            return Some(url);
                        }
                    }
                } else if role == "Pane" {
                    // Recursively search Pane
                    if let Some(url) = Self::search_element_for_url(&child) {
                        return Some(url);
                    }
                }
            }
        }

        None
    }

    /// Extract URL from window title if possible
    fn extract_url_from_title(title: &str) -> Option<String> {
        // Look for common URL patterns in title
        if title.contains("http://") || title.contains("https://") {
            // Extract the URL part
            for part in title.split_whitespace() {
                if part.starts_with("http://") || part.starts_with("https://") {
                    return Some(part.to_string());
                }
            }
        }

        // Try to extract domain from patterns like "Title - domain.com - Browser"
        let parts: Vec<&str> = title.split(" - ").collect();
        if parts.len() >= 2 {
            for part in &parts[1..] {
                // Check if this part looks like a domain
                if part.contains('.') && !part.contains(' ') {
                    // Skip browser names
                    let part_lower = part.to_lowercase();
                    if !part_lower.contains("chrome")
                        && !part_lower.contains("firefox")
                        && !part_lower.contains("edge")
                        && !part_lower.contains("safari")
                        && !part_lower.contains("brave")
                        && !part_lower.contains("opera")
                    {
                        // Assume https if no protocol
                        if part.starts_with("http://") || part.starts_with("https://") {
                            return Some(part.to_string());
                        } else {
                            return Some(format!("https://{part}"));
                        }
                    }
                }
            }
        }

        None
    }

    /// Initialize common hotkey patterns
    fn initialize_hotkey_patterns() -> Vec<HotkeyPattern> {
        vec![
            HotkeyPattern {
                action: "Copy".to_string(),
                keys: vec![162, 67], // Ctrl + C
            },
            HotkeyPattern {
                action: "Paste".to_string(),
                keys: vec![162, 86], // Ctrl + V
            },
            HotkeyPattern {
                action: "Cut".to_string(),
                keys: vec![162, 88], // Ctrl + X
            },
            HotkeyPattern {
                action: "Undo".to_string(),
                keys: vec![162, 90], // Ctrl + Z
            },
            HotkeyPattern {
                action: "Redo".to_string(),
                keys: vec![162, 89], // Ctrl + Y
            },
            HotkeyPattern {
                action: "Save".to_string(),
                keys: vec![162, 83], // Ctrl + S
            },
            HotkeyPattern {
                action: "Switch Window".to_string(),
                keys: vec![164, 9], // Alt + Tab
            },
            HotkeyPattern {
                action: "Show Desktop".to_string(),
                keys: vec![91, 68], // Win + D
            },
            HotkeyPattern {
                action: "Task Manager".to_string(),
                keys: vec![162, 160, 27], // Ctrl + Shift + Esc
            },
        ]
    }

    /// Set up comprehensive event listeners
    async fn setup_comprehensive_listeners(
        &mut self,
        handle: tokio::runtime::Handle,
    ) -> Result<()> {
        // Main input event listener (enhanced from original)
        self.setup_enhanced_input_listener().await?;

        // Clipboard monitoring
        if self.config.record_clipboard {
            self.setup_clipboard_monitor()?;
        }

        // UI Automation event monitoring
        self.setup_ui_automation_events(
            Arc::clone(&self.current_application),
            Arc::clone(&self.browser_tab_tracker),
            Arc::clone(&self.current_text_input),
            Arc::clone(&self.alt_tab_tracker),
            handle,
        )?;

        Ok(())
    }

    /// Set up enhanced input event listener
    async fn setup_enhanced_input_listener(&mut self) -> Result<()> {
        let event_tx = self.event_tx.clone();
        let last_mouse_pos = Arc::clone(&self.last_mouse_pos);
        let stop_indicator_clone = Arc::clone(&self.stop_indicator);
        let is_stopping_clone = Arc::clone(&self.is_stopping);
        let modifier_states = Arc::clone(&self.modifier_states);
        let last_mouse_move_time = Arc::clone(&self.last_mouse_move_time);
        let hotkey_patterns = Arc::clone(&self.hotkey_patterns);
        let config = self.config.clone();
        let performance_last_event_time = Arc::clone(&self.last_event_time);
        let performance_events_counter = Arc::clone(&self.events_this_second);
        let current_text_input = Arc::clone(&self.current_text_input);
        let alt_tab_tracker = Arc::clone(&self.alt_tab_tracker);

        // --- UIA Processor Thread ---
        // Create a channel for rdev events that need UIA processing
        let (uia_event_tx, uia_event_rx) = std::sync::mpsc::channel::<UIAInputRequest>();

        // Spawn the UI Automation processing thread for rdev events
        let uia_processor_event_tx = self.event_tx.clone();
        let uia_processor_config = self.config.clone();
        let uia_processor_text_input = Arc::clone(&self.current_text_input);
        let uia_processor_last_event_time = Arc::clone(&self.last_event_time);
        let uia_processor_events_counter = Arc::clone(&self.events_this_second);
        let uia_processor_is_stopping = Arc::clone(&self.is_stopping);
        let capture_ui_elements = self.config.capture_ui_elements;
        let uia_processor_double_click_tracker = Arc::clone(&self.double_click_tracker);
        let uia_processor_browser_recorder = Arc::clone(&self.browser_recorder);
        let uia_processor_tokio_runtime = Arc::clone(&self.tokio_runtime);

        thread::spawn(move || {
            if !capture_ui_elements {
                return; // Don't start this thread if UI elements are not needed.
            }

            info!("Γ£à UIA processor thread for input events started.");

            // Process events from the rdev listener
            for event_request in uia_event_rx {
                match event_request {
                    UIAInputRequest::ButtonPress { button, position } => {
                        let ctx = ButtonPressContext {
                            position: &position,
                            config: &uia_processor_config,
                            current_text_input: &uia_processor_text_input,
                            event_tx: &uia_processor_event_tx,
                            performance_last_event_time: &uia_processor_last_event_time,
                            performance_events_counter: &uia_processor_events_counter,
                            is_stopping: &uia_processor_is_stopping,
                            double_click_tracker: &uia_processor_double_click_tracker,
                            browser_recorder: &uia_processor_browser_recorder,
                            tokio_runtime: &uia_processor_tokio_runtime,
                        };
                        Self::handle_button_press_request(button, &ctx);
                    }
                    UIAInputRequest::ButtonRelease { button, position } => {
                        Self::handle_button_release_request(
                            button,
                            &position,
                            &uia_processor_config,
                            &uia_processor_event_tx,
                            &uia_processor_last_event_time,
                            &uia_processor_events_counter,
                            &uia_processor_is_stopping,
                        );
                    }
                    UIAInputRequest::KeyPressForCompletion { key_code } => {
                        Self::handle_key_press_for_completion_request(
                            key_code,
                            &uia_processor_text_input,
                            &uia_processor_event_tx,
                        );
                    }
                    UIAInputRequest::ActivationKeyPress { key_code: _ } => {
                        Self::handle_activation_key_press_request(
                            &uia_processor_config,
                            &uia_processor_event_tx,
                        );
                    }
                }
            }
        });

        // --- Rdev Input Listener Thread ---
        thread::spawn(move || {
            let track_modifiers = config.track_modifier_states;
            let record_hotkeys = config.record_hotkeys;
            let mouse_move_throttle = config.mouse_move_throttle_ms;
            let capture_ui_elements_rdev = config.capture_ui_elements;

            let mut active_keys: HashMap<u32, bool> = HashMap::new();

            if let Err(error) = rdev::listen(move |event: rdev::Event| {
                if stop_indicator_clone.load(Ordering::SeqCst) {
                    return;
                }

                match event.event_type {
                    EventType::KeyPress(key) => {
                        let key_code = key_to_u32(&key);
                        active_keys.insert(key_code, true);

                        // Track keystrokes for text input completion
                        if config.record_text_input_completion {
                            if let Ok(mut tracker) = current_text_input.try_lock() {
                                if let Some(ref mut text_input) = tracker.as_mut() {
                                    text_input.add_keystroke(key_code);
                                    // Don't log here to avoid spam

                                    // Check for completion trigger keys (Enter, Tab)
                                    if key_code == 0x0D || key_code == 0x09 {
                                        // Offload the blocking work to the UIA thread
                                        let request =
                                            UIAInputRequest::KeyPressForCompletion { key_code };
                                        if uia_event_tx.send(request).is_err() {
                                            info!("Failed to send key press completion request to UIA thread");
                                        }
                                    }
                                }
                            }
                        }

                        // If Enter or Space is pressed, treat it as a potential activation
                        if key_code == 0x0D || key_code == 0x20 {
                            let request = UIAInputRequest::ActivationKeyPress { key_code };
                            if uia_event_tx.send(request).is_err() {
                                debug!("Failed to send activation key press request to UIA thread");
                            }
                        }

                        // Update modifier states
                        if track_modifiers {
                            Self::update_modifier_states(&modifier_states, key_code, true);
                        }

                        // Check for hotkeys
                        if record_hotkeys {
                            if let Some(hotkey) =
                                Self::detect_hotkey(&hotkey_patterns, &active_keys)
                            {
                                // Check if this is Alt+Tab specifically
                                if hotkey.action.as_deref() == Some("Switch Window") {
                                    // Mark Alt+Tab as pressed for application switch attribution
                                    if let Ok(mut tracker) = alt_tab_tracker.try_lock() {
                                        tracker.mark_alt_tab_pressed();
                                        debug!("≡ƒöÑ Alt+Tab detected - marking for application switch attribution");
                                    }
                                }

                                let _ = event_tx.send(WorkflowEvent::Hotkey(hotkey));
                            }
                        }

                        let modifiers = if track_modifiers {
                            modifier_states.lock().unwrap().clone()
                        } else {
                            ModifierStates {
                                ctrl: false,
                                alt: false,
                                shift: false,
                                win: false,
                            }
                        };
                        let character = if (32..=126).contains(&key_code) {
                            Some(key_code as u8 as char)
                        } else {
                            None
                        };

                        let keyboard_event = KeyboardEvent {
                            key_code,
                            is_key_down: true,
                            ctrl_pressed: modifiers.ctrl,
                            alt_pressed: modifiers.alt,
                            shift_pressed: modifiers.shift,
                            win_pressed: modifiers.win,
                            character,
                            scan_code: None,
                            metadata: EventMetadata {
                                ui_element: None,
                                timestamp: Some(Self::capture_timestamp()),
                            },
                        };

                        Self::send_filtered_event_static(
                            &event_tx,
                            &config,
                            &performance_last_event_time,
                            &performance_events_counter,
                            &is_stopping_clone,
                            WorkflowEvent::Keyboard(keyboard_event),
                        );
                    }
                    EventType::KeyRelease(key) => {
                        let key_code = key_to_u32(&key);
                        active_keys.remove(&key_code);

                        if track_modifiers {
                            Self::update_modifier_states(&modifier_states, key_code, false);
                        }

                        let modifiers = if track_modifiers {
                            modifier_states.lock().unwrap().clone()
                        } else {
                            ModifierStates {
                                ctrl: false,
                                alt: false,
                                shift: false,
                                win: false,
                            }
                        };

                        let keyboard_event = KeyboardEvent {
                            key_code,
                            is_key_down: false,
                            ctrl_pressed: modifiers.ctrl,
                            alt_pressed: modifiers.alt,
                            shift_pressed: modifiers.shift,
                            win_pressed: modifiers.win,
                            character: None,
                            scan_code: None,
                            metadata: EventMetadata {
                                ui_element: None,
                                timestamp: Some(Self::capture_timestamp()),
                            },
                        };
                        Self::send_filtered_event_static(
                            &event_tx,
                            &config,
                            &performance_last_event_time,
                            &performance_events_counter,
                            &is_stopping_clone,
                            WorkflowEvent::Keyboard(keyboard_event),
                        );
                    }
                    EventType::ButtonPress(button) => {
                        if let Some((x, y)) = *last_mouse_pos.lock().unwrap() {
                            let mouse_button = match button {
                                Button::Left => MouseButton::Left,
                                Button::Right => MouseButton::Right,
                                Button::Middle => MouseButton::Middle,
                                _ => return,
                            };
                            let position = Position { x, y };

                            if capture_ui_elements_rdev {
                                let request = UIAInputRequest::ButtonPress {
                                    button: mouse_button,
                                    position,
                                };
                                let _ = uia_event_tx.send(request);
                            } else {
                                let mouse_event = MouseEvent {
                                    event_type: MouseEventType::Down,
                                    button: mouse_button,
                                    position,
                                    scroll_delta: None,
                                    drag_start: None,
                                    metadata: EventMetadata {
                                        ui_element: None,
                                        timestamp: Some(Self::capture_timestamp()),
                                    },
                                };
                                Self::send_filtered_event_static(
                                    &event_tx,
                                    &config,
                                    &performance_last_event_time,
                                    &performance_events_counter,
                                    &is_stopping_clone,
                                    WorkflowEvent::Mouse(mouse_event),
                                );
                            }
                        }
                    }
                    EventType::ButtonRelease(button) => {
                        if let Some((x, y)) = *last_mouse_pos.lock().unwrap() {
                            let mouse_button = match button {
                                Button::Left => MouseButton::Left,
                                Button::Right => MouseButton::Right,
                                Button::Middle => MouseButton::Middle,
                                _ => return,
                            };
                            let position = Position { x, y };

                            if capture_ui_elements_rdev {
                                let request = UIAInputRequest::ButtonRelease {
                                    button: mouse_button,
                                    position,
                                };
                                let _ = uia_event_tx.send(request);
                            } else {
                                let mouse_event = MouseEvent {
                                    event_type: MouseEventType::Up,
                                    button: mouse_button,
                                    position,
                                    scroll_delta: None,
                                    drag_start: None,
                                    metadata: EventMetadata {
                                        ui_element: None,
                                        timestamp: Some(Self::capture_timestamp()),
                                    },
                                };
                                Self::send_filtered_event_static(
                                    &event_tx,
                                    &config,
                                    &performance_last_event_time,
                                    &performance_events_counter,
                                    &is_stopping_clone,
                                    WorkflowEvent::Mouse(mouse_event),
                                );
                            }
                        }
                    }
                    EventType::MouseMove { x, y } => {
                        let x = x as i32;
                        let y = y as i32;
                        *last_mouse_pos.lock().unwrap() = Some((x, y));

                        let now = Instant::now();
                        let should_record = {
                            let mut last_time = last_mouse_move_time.lock().unwrap();
                            if now.duration_since(*last_time).as_millis()
                                >= mouse_move_throttle as u128
                            {
                                *last_time = now;
                                true
                            } else {
                                false
                            }
                        };

                        if should_record && config.record_mouse {
                            let position = Position { x, y };

                            let mouse_event = MouseEvent {
                                event_type: MouseEventType::Move,
                                button: MouseButton::Left,
                                position,
                                scroll_delta: None,
                                drag_start: None,
                                metadata: EventMetadata {
                                    ui_element: None,
                                    timestamp: Some(Self::capture_timestamp()),
                                },
                            };
                            Self::send_filtered_event_static(
                                &event_tx,
                                &config,
                                &performance_last_event_time,
                                &performance_events_counter,
                                &is_stopping_clone,
                                WorkflowEvent::Mouse(mouse_event),
                            );
                        }
                    }
                    EventType::Wheel { delta_x, delta_y } => {
                        if let Some((x, y)) = *last_mouse_pos.lock().unwrap() {
                            let position = Position { x, y };

                            // Capture UI element at scroll position (following pattern from MouseUp events)
                            let ui_element = if config.capture_ui_elements
                                && !config.filter_mouse_noise
                            {
                                Self::get_element_from_point_with_timeout(&config, position, 1000)
                            } else {
                                None
                            };

                            let mouse_event = MouseEvent {
                                event_type: MouseEventType::Wheel,
                                button: MouseButton::Middle, // Common for wheel
                                position,
                                scroll_delta: Some((delta_x as i32, delta_y as i32)),
                                drag_start: None,
                                metadata: EventMetadata {
                                    ui_element,
                                    timestamp: Some(Self::capture_timestamp()),
                                },
                            };
                            Self::send_filtered_event_static(
                                &event_tx,
                                &config,
                                &performance_last_event_time,
                                &performance_events_counter,
                                &is_stopping_clone,
                                WorkflowEvent::Mouse(mouse_event),
                            );
                        }
                    }
                }
            }) {
                error!("Failed to listen for events: {:?}", error);
            }
        });

        Ok(())
    }

    /// Update modifier key states
    fn update_modifier_states(states: &Arc<Mutex<ModifierStates>>, key_code: u32, pressed: bool) {
        let mut states = states.lock().unwrap();
        match key_code {
            162 | 163 => states.ctrl = pressed,  // Left/Right Ctrl
            164 | 165 => states.alt = pressed,   // Left/Right Alt
            160 | 161 => states.shift = pressed, // Left/Right Shift
            91 | 92 => states.win = pressed,     // Left/Right Win
            _ => {}
        }
    }

    /// Detect hotkey combinations
    fn detect_hotkey(
        patterns: &[HotkeyPattern],
        active_keys: &HashMap<u32, bool>,
    ) -> Option<HotkeyEvent> {
        for pattern in patterns {
            if pattern
                .keys
                .iter()
                .all(|&key| active_keys.get(&key).copied().unwrap_or(false))
            {
                return Some(HotkeyEvent {
                    combination: format!("{:?}", pattern.keys), // TODO: Format properly
                    action: Some(pattern.action.clone()),
                    is_global: true,
                    metadata: EventMetadata {
                        ui_element: None,
                        timestamp: Some(Self::capture_timestamp()),
                    }, // TODO: Pass UI element context from caller
                });
            }
        }
        None
    }

    /// Set up clipboard monitoring
    fn setup_clipboard_monitor(&self) -> Result<()> {
        let event_tx = self.event_tx.clone();
        let stop_indicator = Arc::clone(&self.stop_indicator);
        let last_hash = Arc::clone(&self.last_clipboard_hash);
        let config = self.config.clone();
        let capture_ui_elements = self.config.capture_ui_elements;

        thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    error!("Failed to initialize clipboard: {}", e);
                    return;
                }
            };

            // Initialize the clipboard hash with current content to avoid false initial events
            if let Ok(initial_content) = clipboard.get_text() {
                let initial_hash = Self::calculate_hash(&initial_content);
                *last_hash.lock().unwrap() = Some(initial_hash);
                debug!("Initialized clipboard monitoring with existing content hash");
            }

            while !stop_indicator.load(Ordering::SeqCst) {
                if let Ok(content) = clipboard.get_text() {
                    let hash = Self::calculate_hash(&content);
                    let mut last_hash_guard = last_hash.lock().unwrap();

                    if last_hash_guard.as_ref() != Some(&hash) {
                        *last_hash_guard = Some(hash);
                        drop(last_hash_guard);

                        let (truncated_content, truncated) =
                            if content.len() > config.max_clipboard_content_length {
                                (
                                    content[..config.max_clipboard_content_length].to_string(),
                                    true,
                                )
                            } else {
                                (content.clone(), false)
                            };

                        // Capture UI element if enabled
                        let ui_element = if capture_ui_elements {
                            Self::get_focused_ui_element_with_timeout(&config, 200)
                        } else {
                            None
                        };

                        let clipboard_event = ClipboardEvent {
                            action: ClipboardAction::Copy, // Assume copy for content changes
                            content: Some(truncated_content),
                            content_size: Some(content.len()),
                            format: Some("text".to_string()),
                            truncated,
                            metadata: EventMetadata {
                                ui_element,
                                timestamp: Some(Self::capture_timestamp()),
                            },
                        };

                        let _ = event_tx.send(WorkflowEvent::Clipboard(clipboard_event));
                    }
                }

                thread::sleep(Duration::from_millis(200)); // PERFORMANCE: Check clipboard every 200ms (was 100ms)
            }
        });

        Ok(())
    }

    /// Calculate hash for content comparison
    fn calculate_hash(content: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Get focused UI element with a hard timeout to prevent hanging.
    fn get_focused_ui_element_with_timeout(
        config: &WorkflowRecorderConfig,
        timeout_ms: u64,
    ) -> Option<UIElement> {
        let (tx, rx) = std::sync::mpsc::channel();
        let config_clone = config.clone();

        // Spawn a thread to do the blocking UIA work.
        thread::spawn(move || {
            let result = (|| {
                let automation = Self::create_configured_automation_instance(&config_clone).ok()?;
                let element = automation.get_focused_element().ok()?;
                Some(convert_uiautomation_element_to_terminator(element))
            })();
            // The receiver might have timed out and disconnected, so `send` can fail.
            // We can ignore the result.
            let _ = tx.send(result);
        });

        match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(Some(element)) => Some(element),
            Ok(None) => None,
            Err(_) => {
                debug!(
                    "UIA call to get focused element timed out after {}ms.",
                    timeout_ms
                );
                None
            }
        }
    }

    /// Set up UI Automation event handlers
    fn setup_ui_automation_events(
        &self,
        current_application: Arc<Mutex<Option<ApplicationState>>>,
        browser_tab_tracker: Arc<Mutex<BrowserTabTracker>>,
        current_text_input: Arc<Mutex<Option<TextInputTracker>>>,
        alt_tab_tracker: Arc<Mutex<AltTabTracker>>,
        handle: tokio::runtime::Handle,
    ) -> Result<()> {
        let event_tx = self.event_tx.clone();
        let stop_indicator = Arc::clone(&self.stop_indicator);
        let ui_automation_thread_id = Arc::clone(&self.ui_automation_thread_id);

        // Clone filtering configuration
        let ignore_focus_patterns = self.config.ignore_focus_patterns.clone();
        let ignore_window_titles = self.config.ignore_window_titles.clone();
        let ignore_applications = self.config.ignore_applications.clone();
        let config_clone = self.config.clone();

        thread::spawn(move || {
            info!("Starting UI Automation event monitoring thread");

            // Initialize COM apartment for UI Automation events
            // Threading model controlled by configuration
            let threading_model = if config_clone.enable_multithreading {
                COINIT_MULTITHREADED
            } else {
                COINIT_APARTMENTTHREADED
            };

            let com_initialized = unsafe {
                let hr = CoInitializeEx(None, threading_model);
                if hr.is_ok() {
                    let threading_name = if config_clone.enable_multithreading {
                        "multithreaded (MTA)"
                    } else {
                        "apartment threaded (STA)"
                    };
                    info!(
                        "Γ£à Successfully initialized COM apartment as {} for UI Automation events",
                        threading_name
                    );
                    true
                } else if hr == windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                    warn!(
                        "ΓÜá∩╕Å  COM apartment already initialized with different threading model"
                    );
                    // This is expected if the main process already initialized COM differently
                    false
                } else {
                    error!(
                        "Γ¥î Failed to initialize COM apartment for UI Automation: {:?}",
                        hr
                    );
                    return;
                }
            };

            // Store the thread ID for cleanup
            let thread_id = unsafe { GetCurrentThreadId() };
            *ui_automation_thread_id.lock().unwrap() = Some(thread_id);

            info!(
                "UI Automation event thread started (Thread ID: {})",
                thread_id
            );

            // Use new_direct() to avoid COM initialization conflicts
            // The uiautomation library's new() method tries to initialize COM as MTA which conflicts with our STA setup
            let automation = match uiautomation::UIAutomation::new_direct() {
                Ok(auto) => {
                    info!("Γ£à Successfully created UIAutomation instance using new_direct()");
                    auto
                }
                Err(e) => {
                    error!("Γ¥î Failed to create UIAutomation instance: {}", e);
                    warn!(
                        "UI Automation events will be disabled, but other recording will continue"
                    );

                    // Still run message pump for potential future use
                    Self::run_message_pump(&stop_indicator);

                    // Clean up COM if we initialized it
                    if com_initialized {
                        unsafe {
                            CoUninitialize();
                        }
                    }
                    return;
                }
            };

            info!("UI Automation instance created successfully, setting up event handlers");

            // Set up focus change event handler if enabled
            info!("Setting up focus change event handler");
            let focus_event_tx = event_tx.clone();

            // Create a channel to signal focus changes, without sending data to avoid blocking.
            let (focus_tx, focus_rx) = std::sync::mpsc::channel::<()>();

            struct FocusHandler {
                sender: std::sync::mpsc::Sender<()>,
            }

            impl uiautomation::events::CustomFocusChangedEventHandler for FocusHandler {
                fn handle(&self, _sender: &uiautomation::UIElement) -> uiautomation::Result<()> {
                    // This handler is on a critical UIA thread.
                    // DO NOT perform any blocking operations here.
                    // Just send a signal to the processor thread to do the work.
                    self.sender.send(()).ok(); // Disregard error if receiver is gone.
                    Ok(())
                }
            }

            let focus_handler = FocusHandler { sender: focus_tx };

            let focus_event_handler =
                uiautomation::events::UIFocusChangedEventHandler::from(focus_handler);

            // Register the focus change event handler
            match automation.add_focus_changed_event_handler(None, &focus_event_handler) {
                Ok(_) => info!("Γ£à Focus change event handler registered successfully"),
                Err(e) => error!("Γ¥î Failed to register focus change event handler: {}", e),
            }

            // This thread receives signals and performs the blocking UI Automation work safely.
            let focus_event_tx_clone = focus_event_tx.clone();
            let focus_current_app = Arc::clone(&current_application);
            let focus_browser_tracker = Arc::clone(&browser_tab_tracker);
            let focus_current_text_input = Arc::clone(&current_text_input);
            let focus_alt_tab_tracker = Arc::clone(&alt_tab_tracker);

            let focus_processing_config = config_clone.clone();
            let focus_processing_ignore_patterns = ignore_focus_patterns.clone();
            let focus_processing_ignore_window_titles = ignore_window_titles.clone();
            let focus_processing_ignore_applications = ignore_applications.clone();
            let processing_handle = handle;

            std::thread::spawn(move || {
                while focus_rx.recv().is_ok() {
                    // Received a signal. Now, get the currently focused element.
                    // This is the main blocking call, now safely on a dedicated thread.
                    let ui_element =
                        Self::get_focused_ui_element_with_timeout(&focus_processing_config, 200);

                    if let Some(element) = ui_element {
                        let element_name = element.name_or_empty();
                        let element_role = element.role().to_lowercase();
                        debug!(
                            "Focus event received for element: '{}', role: '{}'",
                            element_name, element_role
                        );

                        // Task for application switch check
                        let app_switch_current_app = Arc::clone(&focus_current_app);
                        let app_switch_event_tx_clone = focus_event_tx_clone.clone();
                        let app_switch_element_name = element_name.clone();
                        let app_switch_ui_element = Some(element.clone());
                        let app_switch_config_clone = focus_processing_config.clone();
                        let app_switch_ignore_focus_patterns =
                            focus_processing_ignore_patterns.clone();
                        let app_switch_ignore_window_titles =
                            focus_processing_ignore_window_titles.clone();
                        let app_switch_ignore_applications =
                            focus_processing_ignore_applications.clone();
                        let app_switch_alt_tab_tracker = Arc::clone(&focus_alt_tab_tracker);
                        let app_switch_browser_tracker = Arc::clone(&focus_browser_tracker);

                        processing_handle.spawn(async move {
                            if WindowsRecorder::should_ignore_focus_event(
                                &app_switch_element_name,
                                &app_switch_ui_element,
                                &app_switch_ignore_focus_patterns,
                                &app_switch_ignore_window_titles,
                                &app_switch_ignore_applications,
                            ) {
                                debug!(
                                    "Ignoring focus change event for app switch check: {}",
                                    app_switch_element_name
                                );
                                return;
                            }

                            WindowsRecorder::check_and_emit_application_switch(
                                &app_switch_current_app,
                                &app_switch_event_tx_clone,
                                &app_switch_ui_element,
                                ApplicationSwitchMethod::WindowClick,
                                &app_switch_config_clone,
                                &app_switch_alt_tab_tracker,
                                &app_switch_browser_tracker,
                            );
                        });

                        // Task for browser navigation check
                        let browser_nav_tracker = Arc::clone(&focus_browser_tracker);
                        let browser_nav_event_tx_clone = focus_event_tx_clone.clone();
                        let browser_nav_ui_element = Some(element.clone());
                        let browser_nav_config_clone = focus_processing_config.clone();

                        processing_handle.spawn(async move {
                            Self::check_and_emit_browser_navigation(
                                &browser_nav_tracker,
                                &browser_nav_event_tx_clone,
                                &browser_nav_ui_element,
                                &browser_nav_config_clone,
                            )
                            .await;
                        });

                        // Task for text input completion check
                        let text_input_tracker = Arc::clone(&focus_current_text_input);
                        let text_input_event_tx = focus_event_tx_clone.clone();
                        let text_input_element = Some(element);
                        let text_input_config = focus_processing_config.clone();

                        processing_handle.spawn(async move {
                            WindowsRecorder::handle_text_input_focus_change(
                                &text_input_tracker,
                                &text_input_event_tx,
                                &text_input_element,
                                &text_input_config,
                            );
                        });
                    }
                }
            });

            info!("Γ£à UI Automation event handlers setup complete, starting message pump");

            // CRITICAL: Start Windows message pump for COM/UI Automation events
            Self::run_message_pump(&stop_indicator);

            info!("UI Automation event monitoring stopped");

            // Clean up COM if we initialized it
            if com_initialized {
                unsafe {
                    CoUninitialize();
                }
                debug!("COM uninitialized");
            }
        });

        Ok(())
    }

    /// Run the Windows message pump for UI Automation events
    fn run_message_pump(stop_indicator: &Arc<AtomicBool>) {
        info!("Starting Windows message pump for UI Automation events");
        unsafe {
            let mut msg = MSG::default();
            while !stop_indicator.load(Ordering::SeqCst) {
                let result = GetMessageW(&mut msg, None, 0, 0);

                match result.0 {
                    -1 => {
                        // Error occurred
                        error!("Error in message pump: GetMessage failed");
                        break;
                    }
                    0 => {
                        // WM_QUIT received
                        debug!("WM_QUIT received in UI Automation message pump");
                        break;
                    }
                    _ => {
                        // Normal message - process it
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);

                        // Check for quit message
                        if msg.message == WM_QUIT {
                            debug!("WM_QUIT message processed");
                            break;
                        }
                    }
                }

                // Brief yield to check stop condition more frequently
                if msg.message == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        info!("Windows message pump stopped");
    }

    /// Check if a focus change event should be ignored based on filtering patterns
    fn should_ignore_focus_event(
        element_name: &str,
        ui_element: &Option<UIElement>,
        ignore_patterns: &std::collections::HashSet<String>,
        ignore_window_titles: &std::collections::HashSet<String>,
        ignore_applications: &std::collections::HashSet<String>,
    ) -> bool {
        let element_name_lower = element_name.to_lowercase();

        // Check against focus-specific ignore patterns
        if ignore_patterns
            .iter()
            .any(|pattern| element_name_lower.contains(pattern))
        {
            return true;
        }

        // Check against window title patterns
        if ignore_window_titles
            .iter()
            .any(|title| element_name_lower.contains(title))
        {
            return true;
        }

        // Check against application patterns
        if let Some(ui_elem) = ui_element {
            let app_name = ui_elem.application_name();
            if !app_name.is_empty() {
                let app_name_lower = app_name.to_lowercase();
                if ignore_applications
                    .iter()
                    .any(|app| app_name_lower.contains(app))
                {
                    return true;
                }
            }
        }

        false
    }

    /// Stop recording
    pub fn stop(&self) -> Result<()> {
        debug!("Stopping comprehensive Windows recorder...");

        // CRITICAL: Set is_stopping flag FIRST to prevent new events from being added
        self.is_stopping.store(true, Ordering::SeqCst);

        // Then set stop_indicator to signal event listeners to exit
        self.stop_indicator.store(true, Ordering::SeqCst);

        // Signal the UI Automation thread to stop by posting a quit message
        if let Some(thread_id) = *self.ui_automation_thread_id.lock().unwrap() {
            unsafe {
                let result = PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                if result.is_ok() {
                    debug!(
                        "Posted WM_QUIT message to UI Automation thread {}",
                        thread_id
                    );
                } else {
                    warn!(
                        "Failed to post WM_QUIT message to UI Automation thread {}",
                        thread_id
                    );
                }
            }
        }

        // NOTE: We don't sleep here anymore because:
        // 1. This is a sync function called from async context - blocking sleep causes deadlocks
        // 2. The async delay is handled by caller (WorkflowRecorder::stop and server.rs)
        // 3. The is_stopping flag prevents new events immediately

        info!("Windows recorder stopped - event collection terminated");
        Ok(())
    }

    /// Determine the type of button interaction based on element characteristics
    fn determine_button_interaction_type(
        name: &str,
        description: &str,
        role: &str,
    ) -> ButtonInteractionType {
        let name_lower = name.to_lowercase();
        let desc_lower = description.to_lowercase();
        let role_lower = role.to_lowercase();

        // Check for hyperlinks/links first
        if role_lower.contains("hyperlink") || role_lower.contains("link") {
            return ButtonInteractionType::Click; // Hyperlinks are just clicks
        }

        // Check for dropdown indicators
        if name_lower.contains("dropdown")
            || name_lower.contains("Γû╝")
            || name_lower.contains("ΓÅ╖")
            || desc_lower.contains("dropdown")
            || desc_lower.contains("expand")
            || desc_lower.contains("collapse")
        {
            return ButtonInteractionType::DropdownToggle;
        }

        // Check for submit buttons
        if name_lower.contains("submit")
            || name_lower.contains("save")
            || name_lower.contains("ok")
            || name_lower.contains("apply")
            || name_lower.contains("confirm")
        {
            return ButtonInteractionType::Submit;
        }

        // Check for cancel buttons
        if name_lower.contains("cancel")
            || name_lower.contains("close")
            || name_lower.contains("├ù")
            || name_lower.contains("dismiss")
        {
            return ButtonInteractionType::Cancel;
        }

        // Check for toggle elements
        if role_lower.contains("toggle")
            || role_lower.contains("checkbox")
            || role_lower.contains("radiobutton")
            || name_lower.contains("toggle")
            || desc_lower.contains("toggle")
        {
            return ButtonInteractionType::Toggle;
        }

        // Default to simple click
        ButtonInteractionType::Click
    }

    /// Maps a browser keyword (like 'chrome') to a display name (like 'Google Chrome').
    fn map_keyword_to_browser_name(keyword: &str) -> String {
        match keyword.to_lowercase().as_str() {
            "chrome" | "google chrome" => "Google Chrome".to_string(),
            "firefox" | "mozilla firefox" => "Mozilla Firefox".to_string(),
            "edge" | "msedge" | "microsoft edge" => "Microsoft Edge".to_string(),
            "iexplore" | "internet explorer" => "Internet Explorer".to_string(),
            "safari" => "Safari".to_string(),
            "opera" => "Opera".to_string(),
            other => {
                // Capitalize the first letter as a fallback
                let mut c = other.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            }
        }
    }

    /// Static version for use in event listeners where self is not available
    fn send_filtered_event_static(
        event_tx: &broadcast::Sender<WorkflowEvent>,
        config: &WorkflowRecorderConfig,
        last_event_time: &Arc<Mutex<Instant>>,
        events_this_second: &Arc<Mutex<(u32, Instant)>>,
        is_stopping: &Arc<AtomicBool>,
        event: WorkflowEvent,
    ) {
        // CRITICAL: Check if recorder is stopping - if so, reject all events immediately
        if is_stopping.load(Ordering::SeqCst) {
            return;
        }

        // Apply rate limiting first
        if let Some(max_events) = config.effective_max_events_per_second() {
            let mut counter = events_this_second.lock().unwrap();
            let now = Instant::now();

            // Reset counter if a new second has started
            if now.duration_since(counter.1).as_secs() >= 1 {
                counter.0 = 0;
                counter.1 = now;
            }

            if counter.0 >= max_events {
                return; // Rate limit exceeded
            }

            counter.0 += 1;
        }

        // Apply processing delay
        let processing_delay = config.effective_processing_delay_ms();
        if processing_delay > 0 {
            let mut last_time = last_event_time.lock().unwrap();
            let now = Instant::now();
            if now.duration_since(*last_time).as_millis() < processing_delay as u128 {
                return; // Filter out if within delay window
            }
            *last_time = now;
        }

        // Apply event-specific filtering
        let should_filter = match &event {
            WorkflowEvent::Mouse(mouse_event) => {
                if config.should_filter_mouse_noise() {
                    matches!(
                        mouse_event.event_type,
                        MouseEventType::Move | MouseEventType::Wheel
                    )
                } else {
                    false
                }
            }
            WorkflowEvent::Keyboard(keyboard_event) => {
                if config.should_filter_keyboard_noise() {
                    // Filter key-down events and non-printable keys
                    if keyboard_event.is_key_down {
                        // Keep printable characters (32-126) and common editing keys
                        !((keyboard_event.key_code >= 32 && keyboard_event.key_code <= 126)
                            || matches!(
                                keyboard_event.key_code,
                                0x08 | // Backspace
                            0x2E | // Delete
                            0x20 | // Space
                            0x0D | // Enter
                            0x09 // Tab
                            ))
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            // Never filter high-value events
            WorkflowEvent::ApplicationSwitch(_)
            | WorkflowEvent::Click(_)
            | WorkflowEvent::Clipboard(_) => false,

            // Other events can be filtered in LowEnergy mode
            _ => matches!(config.performance_mode, crate::PerformanceMode::LowEnergy),
        };

        if !should_filter {
            let _ = event_tx.send(event);
        }
    }

    /// Helper function to check if two URLs represent meaningful navigation
    fn is_meaningful_navigation(old_url: &str, new_url: &str) -> bool {
        // Extract base URL without query params and fragments
        fn get_base(url: &str) -> &str {
            url.split('?')
                .next()
                .unwrap_or(url)
                .split('#')
                .next()
                .unwrap_or(url)
        }

        // Compare only base URLs, ignoring all query parameters and fragments
        // Real navigation means the path or domain changed, not just parameters
        get_base(old_url) != get_base(new_url)
    }

    /// Check and emit browser navigation events with improved filtering
    async fn check_and_emit_browser_navigation(
        tracker: &Arc<Mutex<BrowserTabTracker>>,
        event_tx: &broadcast::Sender<WorkflowEvent>,
        ui_element: &Option<UIElement>,
        config: &WorkflowRecorderConfig,
    ) {
        if !config.record_browser_tab_navigation {
            return;
        }

        if let Some(element) = ui_element {
            let app_name = element.application_name();
            let app_name_lower = app_name.to_lowercase();

            let matched_browser_keyword = {
                let tracker_guard = tracker.lock().unwrap();
                tracker_guard
                    .known_browsers
                    .iter()
                    .find(|b| app_name_lower.contains(*b))
                    .cloned()
            };

            debug!(
                "Checking browser navigation for app: '{}', matched browser keyword: {:?}",
                app_name, matched_browser_keyword
            );

            if let Some(keyword) = matched_browser_keyword {
                let browser_display_name = Self::map_keyword_to_browser_name(&keyword);

                // Try multiple methods to get URL information
                let url_info = element
                    .url()
                    .or_else(|| {
                        // Try to get URL from element attributes or text
                        if let Ok(text) = element.text(0) {
                            if text.starts_with("http") {
                                Some(text)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        // Try to extract URL from window title (common in browsers)
                        let window_title = element.window_title();
                        if window_title.contains("http") {
                            // Extract URL from title
                            window_title
                                .split_whitespace()
                                .find(|s| s.starts_with("http"))
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    });

                if let Some(new_url) = url_info {
                    // Use window title as page title, fallback to app name
                    let new_title = {
                        let window_title = element.window_title();
                        if window_title.is_empty() {
                            app_name.clone()
                        } else {
                            window_title
                        }
                    };

                    debug!(
                        "Browser navigation detected - URL: '{}', Title: '{}'",
                        new_url, new_title
                    );

                    let mut tracker_guard = tracker.lock().unwrap();

                    let is_switch = match &tracker_guard.current_url {
                        Some(current_url) => {
                            // Check if this is a meaningful navigation, not just query param changes
                            Self::is_meaningful_navigation(current_url, &new_url)
                        }
                        None => {
                            // First time seeing a URL - check if it's a real page or just a blank tab
                            // Common new tab/blank pages to ignore
                            !new_url.is_empty()
                                && !new_url.starts_with("about:blank")
                                && !new_url.starts_with("about:newtab")
                                && !new_url.starts_with("chrome://newtab")
                                && !new_url.starts_with("edge://newtab")
                                && !new_url.starts_with("about:home")
                                && !new_url.contains("://newtab")
                                && !new_url.contains("://new-tab-page")
                        }
                    };

                    if !is_switch && tracker_guard.current_url.is_some() {
                        debug!(
                            "Ignoring non-meaningful navigation: {} -> {} (likely just query params or in-page state change)",
                            tracker_guard.current_url.as_ref().unwrap(),
                            new_url
                        );
                    }

                    debug!(
                        "Is switch: {}, current_url: {:?}, current_title: {:?}",
                        is_switch, tracker_guard.current_url, tracker_guard.current_title
                    );

                    if is_switch {
                        let now = Instant::now();
                        let dwell_time = now
                            .duration_since(tracker_guard.last_navigation_time)
                            .as_millis() as u64;

                        let nav_event = BrowserTabNavigationEvent {
                            action: crate::TabAction::Switched,
                            method: crate::TabNavigationMethod::Other, // Updated from focus change
                            to_url: Some(new_url.clone()),
                            from_url: tracker_guard.current_url.clone(),
                            to_title: Some(new_title.clone()),
                            from_title: tracker_guard.current_title.clone(),
                            browser: browser_display_name.clone(),
                            tab_index: None,
                            total_tabs: None,
                            page_dwell_time_ms: Some(dwell_time),
                            is_back_forward: false,
                            metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                                element.clone(),
                            )),
                        };

                        debug!("Sending browser navigation event: {:?}", nav_event);

                        if event_tx
                            .send(WorkflowEvent::BrowserTabNavigation(nav_event))
                            .is_ok()
                        {
                            debug!("Γ£à Browser navigation event sent successfully");
                            tracker_guard.last_navigation_time = now;
                        } else {
                            debug!("Γ¥î Failed to send browser navigation event");
                        }
                    }

                    // Always update tracker state, regardless of whether we sent an event
                    // This ensures we track the current state even when no navigation occurs
                    tracker_guard.current_browser = Some(browser_display_name);
                    tracker_guard.current_url = Some(new_url);
                    tracker_guard.current_title = Some(new_title);
                } else {
                    debug!(
                        "No URL information found for browser element: '{}'",
                        element.name_or_empty()
                    );
                }
            }
        }
    }

    /// Check if a UI element is a text input field
    fn is_text_input_element(element: &UIElement) -> bool {
        let role = element.role().to_lowercase();

        // Only track actual input fields, not documents or other containers
        // Note: "text" role is for static text content (labels, hyperlinks, etc.), NOT text inputs
        // Actual text inputs use "edit" role in Windows UIA
        role.contains("edit")
            || (role.contains("combobox") && element.is_enabled().unwrap_or(false))
        // Only editable combobox
    }

    /// Get the text value from a UI element
    /// Try to find a recently active text input element for suggestion completion
    fn find_recent_text_input(config: &WorkflowRecorderConfig) -> Option<UIElement> {
        // Try to find the currently focused element first
        if let Some(focused_element) = Self::get_focused_ui_element_with_timeout(config, 200) {
            if Self::is_text_input_element(&focused_element) {
                debug!(
                    "≡ƒÄ» Found focused text input element: '{}'",
                    focused_element.name_or_empty()
                );
                return Some(focused_element);
            }
        }

        debug!("Γ¥î Could not find any recent text input elements using focused element approach");
        None
    }

    /// Handles text input focus changes to detect text input completion
    fn handle_text_input_focus_change(
        current_text_input: &Arc<Mutex<Option<TextInputTracker>>>,
        event_tx: &broadcast::Sender<WorkflowEvent>,
        new_element: &Option<UIElement>,
        config: &WorkflowRecorderConfig,
    ) {
        // Use centralized text input tracking logic
        Self::handle_text_input_transition(
            current_text_input,
            event_tx,
            new_element,
            "focus_change",
            config,
        );
    }

    /// Centralized text input tracking logic to avoid conflicts between mouse and focus handlers
    fn handle_text_input_transition(
        current_text_input: &Arc<Mutex<Option<TextInputTracker>>>,
        event_tx: &broadcast::Sender<WorkflowEvent>,
        new_element: &Option<UIElement>,
        trigger_reason: &str,
        config: &WorkflowRecorderConfig,
    ) {
        if !config.record_text_input_completion {
            return;
        }

        let mut tracker = match current_text_input.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                debug!("Γ¥î Could not lock text input tracker for transition");
                return;
            }
        };

        // Check if the new element is a potential autocomplete suggestion
        let is_potential_autocomplete_element = if let Some(element) = new_element {
            let element_role = element.role().to_lowercase();
            let element_name = element.name_or_empty();

            element_role.contains("listitem")
                || element_role.contains("menuitem")
                || element_role.contains("option")
                || element_role.contains("comboboxitem")
                || element_role.contains("item")
                || element_role.contains("cell")
                // || element_role == "text" // This is too broad and causes incorrect detections
                || element_name.to_lowercase().contains("suggestion")
                || element_name.to_lowercase().contains("complete")
                || element_name.to_lowercase().contains("autocomplete")
                || element_name.to_lowercase().contains("dropdown")
        } else {
            false
        };

        // If we're focusing on a potential autocomplete element, preserve the existing tracker
        if is_potential_autocomplete_element && tracker.is_some() {
            if let Some(element) = new_element {
                debug!(
                    "≡ƒö╜ Focus moved to potential autocomplete element: '{}' (role: '{}') - PRESERVING text input tracker",
                    element.name_or_empty(), element.role()
                );
            }
            return; // Don't modify the tracker when focusing on autocomplete elements
        }

        // Check if we're leaving a text input field
        if let Some(existing_tracker) = tracker.as_ref() {
            let element_name = existing_tracker.element.name_or_empty();

            // Only remove tracker if we're truly leaving for a non-autocomplete element
            let should_remove_tracker = if let Some(element) = new_element {
                Self::is_text_input_element(element) || !is_potential_autocomplete_element
            } else {
                true // Remove if no focus
            };

            if should_remove_tracker {
                debug!(
                    "≡ƒöä Leaving text input field: '{}' (reason: {})",
                    element_name, trigger_reason
                );

                // Take the tracker to check for completion
                let existing_tracker = tracker.take().unwrap();

                // Check if we should emit a completion event
                if existing_tracker.should_emit_completion(trigger_reason) {
                    debug!("Γ£à Should emit completion event for {}", trigger_reason);
                    if let Some(text_event) = existing_tracker.get_completion_event(None) {
                        debug!(
                            "≡ƒöÑ Emitting text input completion event: '{}' (reason: {})",
                            text_event.text_value, trigger_reason
                        );
                        if let Err(e) = event_tx.send(WorkflowEvent::TextInputCompleted(text_event))
                        {
                            debug!("Failed to send text input completed event: {}", e);
                        }
                    } else {
                        debug!("Γ¥î get_completion_event returned None");
                    }
                } else {
                    debug!(
                        "Γ¥î Should NOT emit completion event for {}",
                        trigger_reason
                    );
                }
            } else {
                debug!(
                    "≡ƒö╜ Staying in text input context: '{}' (reason: {})",
                    element_name, trigger_reason
                );
            }
        }

        // Check if the new element is a text input field (and we don't already have a tracker)
        if let Some(element) = new_element {
            let element_name = element.name_or_empty();
            let element_role = element.role();
            debug!(
                "≡ƒöì Checking new element: '{}' (role: '{}') for text input",
                element_name, element_role
            );

            if Self::is_text_input_element(element) {
                // Check if we already have a tracker for this element
                let should_create_new_tracker = if let Some(existing_tracker) = tracker.as_ref() {
                    // Only create new tracker if it's a different element
                    existing_tracker.element.name_or_empty() != element_name
                        || existing_tracker.element.role() != element_role
                } else {
                    true
                };

                if should_create_new_tracker {
                    debug!(
                        "Γ£à New element is a text input field, starting tracking (reason: {})",
                        trigger_reason
                    );
                    // Store the new text input element with current time
                    let mut new_tracker = TextInputTracker::new(element.clone());
                    // Set focus method based on trigger reason
                    // Preserve MouseClick if it was already set
                    let existing_focus_method = tracker.as_ref().map(|t| t.focus_method.clone());
                    new_tracker.focus_method = if existing_focus_method
                        == Some(crate::events::FieldFocusMethod::MouseClick)
                    {
                        crate::events::FieldFocusMethod::MouseClick
                    } else {
                        match trigger_reason {
                            "mouse_click" => crate::events::FieldFocusMethod::MouseClick,
                            "focus_change" => crate::events::FieldFocusMethod::KeyboardNav,
                            _ => crate::events::FieldFocusMethod::Unknown,
                        }
                    };
                    *tracker = Some(new_tracker);
                    debug!(
                        "≡ƒÄ» Started tracking text input: '{}' ({}) with focus method: {:?}",
                        element_name, element_role, trigger_reason
                    );
                } else {
                    debug!(
                        "≡ƒöì Already tracking this text input element: '{}', keeping existing tracker",
                        element_name
                    );
                }
            } else if !Self::is_text_input_element(element) && !is_potential_autocomplete_element {
                debug!(
                    "Γ¥î New element is NOT a text input field: '{}' ({})",
                    element_name, element_role
                );
            }
        } else {
            debug!("≡ƒöì New element is None (no focus)");
        }
    }

    /// Handles a button press request from the input listener thread.
    /// This function performs the UI Automation calls and is expected to run on a dedicated UIA thread.
    fn handle_button_press_request(button: MouseButton, ctx: &ButtonPressContext) {
        // Check for double click first
        let current_time = Instant::now();
        let is_double_click = if let Ok(mut tracker) = ctx.double_click_tracker.try_lock() {
            tracker.is_double_click(button, *ctx.position, current_time)
        } else {
            false
        };

        let ui_element = if ctx.config.capture_ui_elements {
            // Use deepest element finder for more precise click detection
            // Try with 350ms timeout first
            let mut element = Self::get_deepest_element_from_point_with_timeout(ctx.config, *ctx.position, 350);

            // If first attempt failed, retry once with another 350ms
            if element.is_none() {
                debug!("First element capture attempt failed, retrying with 350ms timeout...");
                element = Self::get_deepest_element_from_point_with_timeout(ctx.config, *ctx.position, 350);
            }

            element
        } else {
            None
        };

        // If this is a double click, emit the double click event
        if is_double_click {
            let double_click_event = crate::MouseEvent {
                event_type: crate::MouseEventType::DoubleClick,
                button,
                position: *ctx.position,
                scroll_delta: None,
                drag_start: None,
                metadata: crate::EventMetadata {
                    ui_element: ui_element.clone(),
                    timestamp: Some(Self::capture_timestamp()),
                },
            };

            debug!(
                "≡ƒû▒∩╕Å≡ƒû▒∩╕Å Double click detected: button={:?}, position=({}, {})",
                button, ctx.position.x, ctx.position.y
            );

            Self::send_filtered_event_static(
                ctx.event_tx,
                ctx.config,
                ctx.performance_last_event_time,
                ctx.performance_events_counter,
                ctx.is_stopping,
                WorkflowEvent::Mouse(double_click_event),
            );
        }

        // Debug: Log what UI element we captured at mouse down
        if let Some(ref element) = ui_element {
            debug!(
                "Mouse down captured element: name='{}', role='{}', position=({}, {})",
                element.name_or_empty(),
                element.role(),
                ctx.position.x,
                ctx.position.y
            );
        } else {
            debug!(
                "Mouse down: No UI element captured at position ({}, {})",
                ctx.position.x, ctx.position.y
            );
        }

        // Check if this is a click on a clickable element and emit button event immediately
        if let Some(ref element) = ui_element {
            if button == MouseButton::Left {
                let element_role = element.role().to_lowercase();
                let element_name = element.name_or_empty();

                // Debug: Log all mouse clicks on elements for debugging
                debug!(
                    "≡ƒû▒∩╕Å Mouse click on element: '{}' (role: '{}') - checking if text input...",
                    element_name, element_role
                );

                // Check if this is a click on a text input element and start tracking
                let is_text_input = Self::is_text_input_element(element);
                debug!(
                    "≡ƒöì is_text_input_element('{}', '{}') = {}",
                    element_name, element_role, is_text_input
                );

                if ctx.config.record_text_input_completion && is_text_input {
                    info!(
                        "≡ƒÄ» Detected mouse click on text input element: '{}' (role: '{}') - STARTING TRACKING",
                        element_name, element_role
                    );
                    // Start tracking text input with MouseClick focus method
                    if let Ok(mut tracker) = ctx.current_text_input.try_lock() {
                        let mut new_tracker = TextInputTracker::new(element.clone());
                        new_tracker.focus_method = crate::events::FieldFocusMethod::MouseClick;
                        *tracker = Some(new_tracker);
                        debug!("Started text input tracking with MouseClick focus method");
                    }
                }

                // Enhanced autocomplete/suggestion detection
                let is_suggestion_click = element_role.contains("listitem")
                    || element_role.contains("menuitem")
                    || element_role.contains("option")
                    || element_role.contains("comboboxitem")
                    || element_role.contains("item") // Generic item roles
                    || element_role.contains("cell") // Grid/table cells in dropdowns
                    // || element_role == "text" // Plain text elements in dropdowns - TOO BROAD
                    || element_name.to_lowercase().contains("suggestion")
                    || element_name.to_lowercase().contains("complete")
                    || element_name.to_lowercase().contains("autocomplete")
                    || element_name.to_lowercase().contains("dropdown"); // Common autocomplete patterns

                // Debug logging for suggestion detection
                debug!(
                    "≡ƒöì Checking suggestion click: element='{}', role='{}', is_suggestion={}, config_enabled=disabled",
                    element_name,
                    element_role,
                    is_suggestion_click
                );

                // Text input completion is no longer tracked in simplified version
                if false {
                    debug!(
                        "≡ƒÄ» Detected potential autocomplete/suggestion click: '{}' (role: '{}') - SUGGESTION SELECTED",
                        element_name, element_role
                    );

                    // Check if we have an active text input tracker that might be affected
                    if let Ok(mut tracker) = ctx.current_text_input.try_lock() {
                        debug!(
                            "≡ƒöÆ Successfully locked text input tracker, checking for active tracker..."
                        );
                        if let Some(ref mut text_input) = tracker.as_mut() {
                            debug!(
                                "Γ£à Found active text input tracker for element: '{}'",
                                text_input.element.name_or_empty()
                            );
                            // Mark as having activity (suggestion selection counts as significant input)
                            text_input.has_typing_activity = true;
                            text_input.keystroke_count += 1; // Count suggestion click as one interaction

                            debug!(
                                "≡ƒô¥ Marking text input as having suggestion selection activity (total keystrokes: {})",
                                text_input.keystroke_count
                            );

                            // Give the UI time to update after suggestion click
                            std::thread::sleep(std::time::Duration::from_millis(150));

                            // Emit suggestion completion with updated text value
                            if text_input.should_emit_completion("suggestion_click") {
                                let completion_time = Instant::now();
                                let typing_duration =
                                    completion_time.duration_since(text_input.start_time);

                                // Use the name of the clicked suggestion element as the final text.
                                let suggested_text = element_name.clone();

                                let text_event = crate::TextInputCompletedEvent {
                                    text_value: suggested_text,
                                    field_name: text_input.element.name(),
                                    field_type: text_input.element.role().to_string(),
                                    keystroke_count: text_input.keystroke_count,
                                    typing_duration_ms: typing_duration.as_millis() as u64,
                                    input_method: crate::TextInputMethod::Suggestion,
                                    focus_method: text_input.focus_method.clone(),
                                    metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                                        text_input.element.clone(),
                                    )),
                                };

                                debug!(
                                    "≡ƒöÑ Emitting text input completion for suggestion click: '{}'",
                                    text_event.text_value
                                );
                                if let Err(e) = ctx
                                    .event_tx
                                    .send(WorkflowEvent::TextInputCompleted(text_event))
                                {
                                    debug!("Failed to send text input completion event: {}", e);
                                } else {
                                    debug!("Γ£à Text input completion event sent successfully for suggestion");
                                }

                                // Reset tracker after emitting - clear but keep the element for potential continued typing
                                let element_for_continuation = text_input.element.clone();
                                *tracker = Some(TextInputTracker::new(element_for_continuation));
                                debug!("≡ƒöä Reset text input tracker after suggestion completion but keep tracking the same element");
                            } else {
                                debug!("Γ¥î Should not emit completion for suggestion click");
                            }
                        } else {
                            debug!(
                                "ΓÜá∩╕Å Suggestion click detected but no active text input tracker found"
                            );
                            debug!(
                                "≡ƒÆí Attempting to create temporary tracker for suggestion completion..."
                            );

                            // Try to find the text input element that was recently active
                            // Look for text input elements on the page
                            if let Some(text_element) = Self::find_recent_text_input(ctx.config) {
                                debug!(
                                    "≡ƒöì Found recent text input element: '{}'",
                                    text_element.name_or_empty()
                                );

                                // Create a temporary tracker for this suggestion completion
                                let temp_tracker = TextInputTracker::new(text_element.clone());

                                // Give the UI time to update after suggestion click
                                std::thread::sleep(std::time::Duration::from_millis(150));

                                let completion_time = Instant::now();
                                let typing_duration =
                                    completion_time.duration_since(temp_tracker.start_time);
                                let suggested_text = element_name.clone();

                                let text_event = crate::TextInputCompletedEvent {
                                    text_value: suggested_text,
                                    field_name: temp_tracker.element.name(),
                                    field_type: temp_tracker.element.role().to_string(),
                                    keystroke_count: 1, // just the click
                                    typing_duration_ms: typing_duration.as_millis() as u64,
                                    input_method: crate::TextInputMethod::Suggestion,
                                    focus_method: temp_tracker.focus_method.clone(),
                                    metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                                        temp_tracker.element.clone(),
                                    )),
                                };

                                debug!(
                                    "≡ƒöÑ Emitting text input completion from temp tracker: '{}'",
                                    text_event.text_value
                                );
                                if let Err(e) = ctx
                                    .event_tx
                                    .send(WorkflowEvent::TextInputCompleted(text_event))
                                {
                                    debug!("Failed to send temp tracker completion event: {}", e);
                                } else {
                                    debug!("Γ£à Temp tracker completion event sent successfully");
                                }

                                // Create new tracker for potential continued typing
                                *tracker = Some(TextInputTracker::new(text_element));
                                debug!("≡ƒöä Created new tracker after temp completion");
                            } else {
                                debug!("Γ¥î Could not find recent text input element for suggestion completion");
                            }
                        }
                    } else {
                        debug!("Γ¥î Could not lock text input tracker for suggestion click");
                    }
                }

                // Always emit click event, even for text inputs
                // Both Click and TextInputCompleted events provide valuable information
                // Click shows the interaction happened, TextInputCompleted shows what was typed
                {
                    debug!(
                        "≡ƒû▒∩╕Å Mouse click on element: '{}' (role: '{}') - emitting Click event",
                        element_name, element_role
                    );

                    let element_desc = element.attributes().description.unwrap_or_default();
                    let interaction_type = Self::determine_button_interaction_type(
                        &element_name,
                        &element_desc,
                        &element_role,
                    );

                    // Since we now have the deepest element, collect only direct children (not unlimited depth)
                    let child_text_content = Self::collect_direct_child_text_content(element);
                    info!(
                        "≡ƒöì DIRECT CHILD TEXT COLLECTION: Found {} child elements: {:?}",
                        child_text_content.len(),
                        child_text_content
                    );

                    // Calculate relative position within the element
                    let relative_position = element.bounds().ok().map(|bounds| {
                        let x_ratio =
                            ((ctx.position.x as f64 - bounds.0) / bounds.2).clamp(0.0, 1.0) as f32;
                        let y_ratio =
                            ((ctx.position.y as f64 - bounds.1) / bounds.3).clamp(0.0, 1.0) as f32;
                        (x_ratio, y_ratio)
                    });

                    if let Some((x, y)) = relative_position {
                        debug!(
                            "📍 Click relative position: {:.1}% x {:.1}% within element",
                            x * 100.0,
                            y * 100.0
                        );
                    }

                    // Check if this is a browser click and try to capture DOM information
                    let app_name = element.application_name().to_lowercase();
                    let is_browser = app_name.contains("chrome")
                        || app_name.contains("firefox")
                        || app_name.contains("edge")
                        || app_name.contains("safari");

                    // Try to capture DOM element if in browser
                    let mut dom_element = None;
                    if is_browser {
                        debug!(
                            "🌐 Browser click detected, attempting DOM capture at ({}, {})",
                            ctx.position.x, ctx.position.y
                        );

                        // Use async/sync bridge to query DOM
                        if let Ok(browser_lock) = ctx.browser_recorder.lock() {
                            if let Some(ref browser) = *browser_lock {
                                if let Ok(runtime_lock) = ctx.tokio_runtime.lock() {
                                    if let Some(ref runtime) = *runtime_lock {
                                        let browser_clone = browser.clone();
                                        let position_clone = *ctx.position;

                                        // Create channel for async result
                                        let (tx, rx) = std::sync::mpsc::channel();

                                        // Spawn async task
                                        runtime.spawn(async move {
                                            let result = browser_clone
                                                .capture_dom_element(position_clone)
                                                .await;
                                            let _ = tx.send(result);
                                        });

                                        // Wait for DOM result with timeout
                                        if let Ok(dom_result) =
                                            rx.recv_timeout(Duration::from_millis(200))
                                        {
                                            if let Some(browser_dom_info) = dom_result {
                                                debug!(
                                                    "✅ DOM element captured: {} with {} selectors",
                                                    browser_dom_info.tag_name,
                                                    browser_dom_info.selector_candidates.len()
                                                );
                                                // Convert from browser_context::DomElementInfo to events::DomElementInfo
                                                let converted_dom = crate::events::DomElementInfo {
                                                    tag_name: browser_dom_info.tag_name,
                                                    id: browser_dom_info.id,
                                                    class_names: browser_dom_info.class_names,
                                                    css_selector: browser_dom_info.css_selector,
                                                    xpath: browser_dom_info.xpath,
                                                    inner_text: browser_dom_info.inner_text,
                                                    input_value: browser_dom_info.input_value,
                                                    is_visible: browser_dom_info.is_visible,
                                                    is_interactive: browser_dom_info.is_interactive,
                                                    aria_label: browser_dom_info.aria_label,
                                                    selector_candidates: browser_dom_info
                                                        .selector_candidates
                                                        .into_iter()
                                                        .map(|sc| {
                                                            crate::events::SelectorCandidate {
                                                                selector: sc.selector,
                                                                selector_type: format!(
                                                                    "{:?}",
                                                                    sc.selector_type
                                                                ),
                                                                specificity: sc.specificity,
                                                                requires_jquery: sc.requires_jquery,
                                                            }
                                                        })
                                                        .collect(),
                                                };
                                                dom_element = Some(converted_dom);
                                            } else {
                                                debug!("⚠️ No DOM element found at coordinates");
                                            }
                                        } else {
                                            debug!("⚠️ DOM capture timed out");
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Emit browser click event if DOM was captured, regular click otherwise
                    if let Some(dom_info) = dom_element {
                        // Get page context
                        let (page_url, page_title) =
                            if let Ok(browser_lock) = ctx.browser_recorder.lock() {
                                if let Some(ref browser) = *browser_lock {
                                    if let Ok(runtime_lock) = ctx.tokio_runtime.lock() {
                                        if let Some(ref runtime) = *runtime_lock {
                                            let browser_clone = browser.clone();
                                            let (tx, rx) = std::sync::mpsc::channel();

                                            runtime.spawn(async move {
                                                let result = browser_clone.get_page_context().await;
                                                let _ = tx.send(result);
                                            });

                                            if let Ok(Some(context)) =
                                                rx.recv_timeout(Duration::from_millis(100))
                                            {
                                                (context.url, context.title)
                                            } else {
                                                (String::new(), String::new())
                                            }
                                        } else {
                                            (String::new(), String::new())
                                        }
                                    } else {
                                        (String::new(), String::new())
                                    }
                                } else {
                                    (String::new(), String::new())
                                }
                            } else {
                                (String::new(), String::new())
                            };

                        let browser_click_event = BrowserClickEvent {
                            ui_element: Some(element.clone()),
                            dom_element: Some(dom_info.clone()),
                            position: *ctx.position,
                            selectors: dom_info.selector_candidates.clone(),
                            page_url,
                            page_title,
                            timestamp: Self::capture_timestamp(),
                            button,
                            is_double_click,
                            metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                                element.clone(),
                            )),
                        };

                        debug!("🌐 Emitting BrowserClickEvent with DOM information");
                        if let Err(e) = ctx
                            .event_tx
                            .send(WorkflowEvent::BrowserClick(browser_click_event))
                        {
                            debug!("Failed to send browser click event: {}", e);
                        } else {
                            debug!("✅ Browser click event sent successfully with DOM data");
                        }
                    }

                    // Always emit regular click event as well (for dual recording)
                    let click_event = ClickEvent {
                        element_text: element_name,
                        interaction_type,
                        element_role: element_role.clone(),
                        was_enabled: element.is_enabled().unwrap_or(true),
                        click_position: Some(*ctx.position),
                        element_description: if element_desc.is_empty() {
                            None
                        } else {
                            Some(element_desc)
                        },
                        child_text_content,
                        relative_position,
                        metadata: EventMetadata::with_ui_element_and_timestamp(Some(
                            element.clone(),
                        )),
                    };

                    if let Err(e) = ctx.event_tx.send(WorkflowEvent::Click(click_event)) {
                        debug!("Failed to send click event: {}", e);
                    } else {
                        debug!("Γ£à Click event sent successfully");
                    }
                }
            }
        } else if button == MouseButton::Left {
            // UI element capture failed, but we should still emit a Click event
            // This ensures we don't lose click events during page transitions or when element capture times out
            debug!(
                "⚠️ Mouse click without UI element at position ({}, {}) - still emitting Click event",
                ctx.position.x, ctx.position.y
            );

            let click_event = ClickEvent {
                element_text: String::from("[Element capture failed]"),
                interaction_type: crate::ButtonInteractionType::Click,
                element_role: String::from("unknown"),
                was_enabled: true,
                click_position: Some(*ctx.position),
                element_description: None,
                child_text_content: Vec::new(),
                relative_position: None,
                metadata: EventMetadata {
                    ui_element: None,
                    timestamp: Some(Self::capture_timestamp()),
                },
            };

            if let Err(e) = ctx.event_tx.send(WorkflowEvent::Click(click_event)) {
                debug!("Failed to send click event (no UI element): {}", e);
            } else {
                debug!("✅ Click event sent successfully (no UI element)");
            }
        }

        let mouse_event = MouseEvent {
            event_type: MouseEventType::Down,
            button,
            position: *ctx.position,
            scroll_delta: None,
            drag_start: None,
            metadata: EventMetadata {
                ui_element,
                timestamp: Some(Self::capture_timestamp()),
            },
        };
        Self::send_filtered_event_static(
            ctx.event_tx,
            ctx.config,
            ctx.performance_last_event_time,
            ctx.performance_events_counter,
            ctx.is_stopping,
            WorkflowEvent::Mouse(mouse_event),
        );
    }

    /// Handles a button release request from the input listener thread.
    fn handle_button_release_request(
        button: MouseButton,
        position: &Position,
        config: &WorkflowRecorderConfig,
        event_tx: &broadcast::Sender<WorkflowEvent>,
        performance_last_event_time: &Arc<Mutex<Instant>>,
        performance_events_counter: &Arc<Mutex<(u32, Instant)>>,
        is_stopping: &Arc<AtomicBool>,
    ) {
        let ui_element = if config.capture_ui_elements {
            Self::get_element_from_point_with_timeout(config, *position, 1000)
        } else {
            None
        };

        let mouse_event = MouseEvent {
            event_type: MouseEventType::Up,
            button,
            position: *position,
            scroll_delta: None,
            drag_start: None,
            metadata: EventMetadata {
                ui_element,
                timestamp: Some(Self::capture_timestamp()),
            },
        };
        Self::send_filtered_event_static(
            event_tx,
            config,
            performance_last_event_time,
            performance_events_counter,
            is_stopping,
            WorkflowEvent::Mouse(mouse_event),
        );
    }

    /// Find the deepest/most specific element at the given coordinates.
    /// This drills down through the UI hierarchy to find the smallest element that contains the click point.
    fn get_deepest_element_from_point_with_timeout(
        config: &WorkflowRecorderConfig,
        position: Position,
        timeout_ms: u64,
    ) -> Option<UIElement> {
        let (tx, rx) = std::sync::mpsc::channel();
        let config_clone = config.clone();

        thread::spawn(move || {
            let result = (|| {
                let automation = Self::create_configured_automation_instance(&config_clone).ok()?;
                let point = Point::new(position.x, position.y);
                let element = automation.element_from_point(point).ok()?;
                let terminator_element = convert_uiautomation_element_to_terminator(element);

                // Find the deepest element that contains our click point
                Self::find_deepest_element_at_coordinates(&terminator_element, position)
            })();
            let _ = tx.send(result);
        });

        match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(Some(element)) => Some(element),
            Ok(None) => None,
            Err(_) => {
                debug!(
                    "UIA call to get deepest element from point timed out after {}ms.",
                    timeout_ms
                );
                None
            }
        }
    }

    /// Recursively traverse down the UI hierarchy to find the deepest element containing the coordinates.
    fn find_deepest_element_at_coordinates(
        element: &UIElement,
        position: Position,
    ) -> Option<UIElement> {
        debug!(
            "≡ƒöì Checking element '{}' (role: {}) for coordinates ({}, {})",
            element.name().unwrap_or_default(),
            element.role(),
            position.x,
            position.y
        );

        // Check current element bounds
        if let Ok(bounds) = element.bounds() {
            debug!(
                "   Element bounds: ({}, {}, {}, {})",
                bounds.0, bounds.1, bounds.2, bounds.3
            );

            // If current element doesn't contain our point, return None
            if !(bounds.0 <= position.x as f64
                && position.x as f64 <= bounds.0 + bounds.2
                && bounds.1 <= position.y as f64
                && position.y as f64 <= bounds.1 + bounds.3)
            {
                debug!("   Γ¥î Point is outside element bounds");
                return None;
            }
        } else {
            debug!("   ΓÜá∩╕Å Cannot get element bounds");
        }

        // Try to find a deeper child that contains our point
        if let Ok(children) = element.children() {
            debug!("   Checking {} children for deeper matches", children.len());

            for child in children {
                if let Some(deeper_element) =
                    Self::find_deepest_element_at_coordinates(&child, position)
                {
                    debug!(
                        "   Γ£à Found deeper element: '{}' (role: {})",
                        deeper_element.name().unwrap_or_default(),
                        deeper_element.role()
                    );
                    return Some(deeper_element);
                }
            }
        }

        // Before returning this element, check if it's an empty container
        // with a single child that has content
        let element_name = element.name().unwrap_or_default();
        if element_name.is_empty() {
            if let Ok(children) = element.children() {
                // Check for single child with content
                if children.len() == 1 {
                    let child = &children[0];
                    let child_name = child.name().unwrap_or_default();

                    // If the single child has meaningful content
                    if !child_name.is_empty() {
                        // Verify the child is within our click bounds
                        if let Ok(child_bounds) = child.bounds() {
                            if child_bounds.0 <= position.x as f64
                                && position.x as f64 <= child_bounds.0 + child_bounds.2
                                && child_bounds.1 <= position.y as f64
                                && position.y as f64 <= child_bounds.1 + child_bounds.3
                            {
                                debug!(
                                    "   ≡ƒÄ» Preferring child with content: '{}' (role: {}) over empty parent",
                                    child_name, child.role()
                                );
                                return Some(child.clone());
                            }
                        }
                    }
                }

                // Also check for cases where we have multiple children but click is clearly on one
                // This handles cases like clicking on "1" in a group with multiple text elements
                for child in &children {
                    let child_name = child.name().unwrap_or_default();
                    if !child_name.is_empty() {
                        if let Ok(child_bounds) = child.bounds() {
                            // Check if click is within this specific child's bounds
                            if child_bounds.0 <= position.x as f64
                                && position.x as f64 <= child_bounds.0 + child_bounds.2
                                && child_bounds.1 <= position.y as f64
                                && position.y as f64 <= child_bounds.1 + child_bounds.3
                            {
                                debug!(
                                    "   ≡ƒÄ» Found child with content at click position: '{}' (role: {})",
                                    child_name, child.role()
                                );
                                return Some(child.clone());
                            }
                        }
                    }
                }
            }
        }

        // No deeper element found, this is the deepest one
        debug!(
            "   ≡ƒÄ» Using this element as deepest: '{}' (role: {})",
            element_name,
            element.role()
        );
        Some(element.clone())
    }

    /// Get element from a specific point with a hard timeout (legacy method for compatibility).
    fn get_element_from_point_with_timeout(
        config: &WorkflowRecorderConfig,
        position: Position,
        timeout_ms: u64,
    ) -> Option<UIElement> {
        let (tx, rx) = std::sync::mpsc::channel();
        let config_clone = config.clone();

        thread::spawn(move || {
            let result = (|| {
                let automation = Self::create_configured_automation_instance(&config_clone).ok()?;
                let point = Point::new(position.x, position.y);
                let element = automation.element_from_point(point).ok()?;
                Some(convert_uiautomation_element_to_terminator(element))
            })();
            let _ = tx.send(result);
        });

        match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(Some(element)) => Some(element),
            Ok(None) => None,
            Err(_) => {
                debug!(
                    "UIA call to get element from point timed out after {}ms.",
                    timeout_ms
                );
                None
            }
        }
    }

    /// Handles a key press completion request from the input listener thread.
    /// This function performs the UI Automation calls and is expected to run on a dedicated UIA thread.
    fn handle_key_press_for_completion_request(
        key_code: u32,
        current_text_input: &Arc<Mutex<Option<TextInputTracker>>>,
        event_tx: &broadcast::Sender<WorkflowEvent>,
    ) {
        if let Ok(mut tracker) = current_text_input.lock() {
            if let Some(ref mut text_input) = tracker.as_mut() {
                let is_suggestion_enter = if key_code == 0x0D {
                    text_input.handle_enter_key()
                } else {
                    false
                };
                let completion_reason = if is_suggestion_enter {
                    "suggestion_enter"
                } else {
                    "trigger_key"
                };

                if text_input.should_emit_completion(completion_reason) {
                    let input_method = if is_suggestion_enter {
                        Some(crate::TextInputMethod::Suggestion)
                    } else {
                        None
                    };
                    if let Some(text_event) = text_input.get_completion_event(input_method) {
                        let _ = event_tx.send(WorkflowEvent::TextInputCompleted(text_event));
                        // Reset the tracker to continue tracking on the same element
                        if let Some(element) = &tracker.as_ref().map(|t| t.element.clone()) {
                            *tracker = Some(TextInputTracker::new(element.clone()));
                        }
                    }
                }
            }
        }
    }

    /// Handles an activation key press (Enter/Space) to check for button clicks.
    fn handle_activation_key_press_request(
        config: &WorkflowRecorderConfig,
        event_tx: &broadcast::Sender<WorkflowEvent>,
    ) {
        // Get the currently focused element
        if let Some(element) = Self::get_focused_ui_element_with_timeout(config, 200) {
            let element_name = element.name_or_empty();
            let element_role = element.role().to_lowercase();

            // Check if the focused element is clickable
            if element_role.contains("button")
                || element_role.contains("menuitem")
                || element_role.contains("listitem")
                || element_role.contains("hyperlink")
                || element_role.contains("link")
                || element_role.contains("checkbox")
                || element_role.contains("radiobutton")
                || element_role.contains("togglebutton")
            {
                debug!(
                    "Γ£à Detected clickable element on activation key press: '{}' (role: '{}')",
                    element_name, element_role
                );
                let element_desc = element.attributes().description.unwrap_or_default();

                let interaction_type = Self::determine_button_interaction_type(
                    &element_name,
                    &element_desc,
                    &element_role,
                );
                let is_enabled = element.is_enabled().unwrap_or(true);
                let bounds = element.bounds().unwrap_or_default();

                // Collect child text content with unlimited depth traversal
                let child_text_content = Self::collect_child_text_content(&element);
                info!(
                    "≡ƒöì CHILD TEXT COLLECTION (key press): Found {} child elements: {:?}",
                    child_text_content.len(),
                    child_text_content
                );

                let click_event = ClickEvent {
                    element_text: element_name.clone(),
                    interaction_type,
                    element_role: element_role.clone(),
                    was_enabled: is_enabled,
                    click_position: Some(Position {
                        x: bounds.0 as i32,
                        y: bounds.1 as i32,
                    }),
                    element_description: if element_desc.is_empty() {
                        None
                    } else {
                        Some(element_desc.clone())
                    },
                    child_text_content,
                    relative_position: None, // No relative position for keyboard-triggered clicks
                    metadata: EventMetadata::with_ui_element_and_timestamp(Some(element.clone())),
                };

                if let Err(e) = event_tx.send(WorkflowEvent::Click(click_event)) {
                    debug!(
                        "Failed to send click event from key press for '{}': {}",
                        element_name, e
                    );
                }
            }
        }
    }
}
