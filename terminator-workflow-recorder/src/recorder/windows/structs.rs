use crate::events::EventMetadata;
use rdev::Key;
use std::time::Instant;
use terminator::UIElement;
use tracing::{debug, error};

/// Represents an input event that requires UI Automation processing.
#[derive(Debug)]
pub enum UIAInputRequest {
    ButtonPress {
        button: crate::events::MouseButton,
        position: crate::events::Position,
    },
    ButtonRelease {
        button: crate::events::MouseButton,
        position: crate::events::Position,
    },
    KeyPressForCompletion {
        key_code: u32,
    },
    ActivationKeyPress {
        key_code: u32,
    },
}

/// Text input tracking state
#[derive(Debug, Clone)]
pub struct TextInputTracker {
    /// The UI element being tracked
    pub element: UIElement,
    /// When tracking started
    pub start_time: Instant,
    /// Number of typing keystrokes (excludes navigation keys)
    pub keystroke_count: u32,
    /// Whether we've detected any actual typing
    pub has_typing_activity: bool,
    /// Whether we're in the middle of autocomplete navigation (arrow keys active)
    pub in_autocomplete_navigation: bool,
    /// Last time we detected autocomplete navigation activity
    pub last_autocomplete_activity: Instant,
    /// Text value before autocomplete selection (for change detection)
    pub text_before_autocomplete: Option<String>,
}

impl TextInputTracker {
    pub fn new(element: UIElement) -> Self {
        // Don't try to get initial text to avoid potential access violations
        Self {
            element,
            start_time: Instant::now(),
            keystroke_count: 0,
            has_typing_activity: false,
            in_autocomplete_navigation: false,
            last_autocomplete_activity: Instant::now(),
            text_before_autocomplete: None,
        }
    }

    pub fn add_keystroke(&mut self, key_code: u32) {
        // Check for autocomplete navigation keys (arrow keys, escape)
        if Self::is_autocomplete_navigation_key(key_code) {
            self.in_autocomplete_navigation = true;
            self.last_autocomplete_activity = Instant::now();
            debug!(
                "🔽 Autocomplete navigation detected: key {} (Arrow/Escape)",
                key_code
            );

            // Capture current text value before potential autocomplete selection
            if self.text_before_autocomplete.is_none() {
                self.text_before_autocomplete = Self::get_element_text_value_safe(&self.element);
                debug!(
                    "📝 Captured text before autocomplete: {:?}",
                    self.text_before_autocomplete
                );
            }
            return;
        }

        // Only count actual typing keys, not navigation/modifier keys
        if Self::is_typing_key(key_code) {
            self.keystroke_count += 1;
            self.has_typing_activity = true;
            // Reset autocomplete state on new typing
            self.in_autocomplete_navigation = false;
        }
    }

    fn is_autocomplete_navigation_key(key_code: u32) -> bool {
        matches!(
            key_code,
            0x26 |  // Up arrow
            0x28 |  // Down arrow
            0x25 |  // Left arrow (less common in autocomplete but possible)
            0x27 |  // Right arrow (less common in autocomplete but possible)
            0x1B // Escape (cancel autocomplete)
        )
    }

    fn is_typing_key(key_code: u32) -> bool {
        // Letters, numbers, space, punctuation - actual content input
        matches!(key_code,
            0x30..=0x39 |  // Numbers 0-9
            0x41..=0x5A |  // Letters A-Z
            0x20 |         // Space
            0x08 |         // Backspace
            0x2E |         // Delete
            // Common punctuation and symbols
            0xBA..=0xC0 |  // ;=,-./`
            0xDB..=0xDE    // [\]'
        )
    }

    pub fn handle_enter_key(&mut self) -> bool {
        // If we're in autocomplete navigation, Enter likely selects a suggestion
        if self.in_autocomplete_navigation {
            let time_since_nav = self.last_autocomplete_activity.elapsed();
            if time_since_nav < std::time::Duration::from_millis(5000) {
                // 5 second window
                debug!("🔥 Enter pressed during autocomplete navigation - suggestion selection detected!");
                self.has_typing_activity = true;
                self.keystroke_count += 1; // Count as one interaction
                self.in_autocomplete_navigation = false; // Reset state
                return true; // Indicates this is a suggestion selection
            }
        }
        false
    }

    pub fn should_emit_completion(&self, reason: &str) -> bool {
        // For trigger keys (Enter/Tab), require both activity and keystrokes
        if reason == "trigger_key" || reason == "suggestion_enter" {
            return self.has_typing_activity && self.keystroke_count > 0;
        }

        // For focus changes, be more lenient - emit if there was any activity
        if reason == "focus_change" {
            return self.has_typing_activity || self.keystroke_count > 0;
        }

        // For suggestion clicks, check if we have activity
        if reason == "suggestion_click" {
            return self.has_typing_activity || self.keystroke_count > 0;
        }

        // Default: require activity
        self.has_typing_activity && self.keystroke_count > 0
    }

    #[allow(dead_code)]
    fn text_changed(&self) -> bool {
        // Always return false to avoid accessing element properties
        // We'll rely on keystroke counting instead
        false
    }

    pub fn get_completion_event(
        &self,
        input_method: Option<crate::TextInputMethod>,
    ) -> Option<crate::TextInputCompletedEvent> {
        // Only proceed if we have typing activity
        if !self.has_typing_activity && self.keystroke_count == 0 {
            debug!("❌ No typing activity or keystrokes");
            return None;
        }

        // Try to get actual text value from the element
        let text_value = match self.element.text(0) {
            Ok(actual_text) => actual_text,
            Err(e) => {
                error!("❌ Could not get text value: {}", e);
                String::new()
            }
        };

        // Do not emit an event for empty or whitespace-only text.
        if text_value.trim().is_empty() {
            debug!("❌ Text value is empty or whitespace-only, not emitting completion event.");
            return None;
        }

        let typing_duration_ms = self.start_time.elapsed().as_millis() as u64;

        // Use safe fallbacks for element properties
        let field_name = self.element.name();
        let field_type = self.element.role();

        // Determine input method
        let final_input_method = input_method.unwrap_or(crate::TextInputMethod::Typed);

        Some(crate::TextInputCompletedEvent {
            text_value,
            field_name,
            field_type,
            input_method: final_input_method,
            typing_duration_ms,
            keystroke_count: self.keystroke_count,
            metadata: EventMetadata::with_ui_element_and_timestamp(Some(self.element.clone())),
        })
    }

    fn get_element_text_value_safe(element: &UIElement) -> Option<String> {
        match element.text(0) {
            Ok(text) => Some(text),
            Err(e) => {
                debug!(
                    "Could not safely get element text for autocomplete tracking (this is okay): {}",
                    e
                );
                None
            }
        }
    }
}

/// Modifier key states
#[derive(Debug, Clone)]
pub struct ModifierStates {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

#[derive(Debug, Clone)]
pub struct HotkeyPattern {
    pub action: String,
    pub keys: Vec<u32>,
}

/// Tracks the current application state for switch detection
#[derive(Debug, Clone)]
pub struct ApplicationState {
    /// Application name/title
    pub name: String,
    /// Process ID
    pub process_id: u32,
    /// When the application became active
    pub start_time: Instant,
}

/// Tracks browser tab navigation state
#[derive(Debug, Clone)]
pub struct BrowserTabTracker {
    /// Current browser application
    pub current_browser: Option<String>,
    /// Current URL (best effort detection)
    pub current_url: Option<String>,
    /// Current page title
    pub current_title: Option<String>,
    /// Known browser process names
    pub known_browsers: Vec<String>,
    /// When the current page was last accessed
    pub last_navigation_time: Instant,
}

impl Default for BrowserTabTracker {
    fn default() -> Self {
        Self {
            current_browser: None,
            current_url: None,
            current_title: None,
            known_browsers: vec![
                // Executable names
                "chrome.exe".to_string(),
                "firefox.exe".to_string(),
                "msedge.exe".to_string(),
                "brave.exe".to_string(),
                "opera.exe".to_string(),
                "vivaldi.exe".to_string(),
                "iexplore.exe".to_string(),
                // Display names
                "google chrome".to_string(),
                "chrome".to_string(),
                "firefox".to_string(),
                "mozilla firefox".to_string(),
                "microsoft edge".to_string(),
                "edge".to_string(),
                "brave".to_string(),
                "opera".to_string(),
                "vivaldi".to_string(),
                "internet explorer".to_string(),
            ],
            last_navigation_time: Instant::now(),
        }
    }
}

/// Convert a Key to a u32
pub fn key_to_u32(key: &Key) -> u32 {
    match key {
        Key::KeyA => 0x41,
        Key::KeyB => 0x42,
        Key::KeyC => 0x43,
        Key::KeyD => 0x44,
        Key::KeyE => 0x45,
        Key::KeyF => 0x46,
        Key::KeyG => 0x47,
        Key::KeyH => 0x48,
        Key::KeyI => 0x49,
        Key::KeyJ => 0x4A,
        Key::KeyK => 0x4B,
        Key::KeyL => 0x4C,
        Key::KeyM => 0x4D,
        Key::KeyN => 0x4E,
        Key::KeyO => 0x4F,
        Key::KeyP => 0x50,
        Key::KeyQ => 0x51,
        Key::KeyR => 0x52,
        Key::KeyS => 0x53,
        Key::KeyT => 0x54,
        Key::KeyU => 0x55,
        Key::KeyV => 0x56,
        Key::KeyW => 0x57,
        Key::KeyX => 0x58,
        Key::KeyY => 0x59,
        Key::KeyZ => 0x5A,
        Key::Num0 => 0x30,
        Key::Num1 => 0x31,
        Key::Num2 => 0x32,
        Key::Num3 => 0x33,
        Key::Num4 => 0x34,
        Key::Num5 => 0x35,
        Key::Num6 => 0x36,
        Key::Num7 => 0x37,
        Key::Num8 => 0x38,
        Key::Num9 => 0x39,
        Key::Escape => 0x1B,
        Key::Backspace => 0x08,
        Key::Tab => 0x09,
        Key::Return => 0x0D,
        Key::Space => 0x20,
        Key::LeftArrow => 0x25,
        Key::UpArrow => 0x26,
        Key::RightArrow => 0x27,
        Key::DownArrow => 0x28,
        Key::Delete => 0x2E,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::F1 => 0x70,
        Key::F2 => 0x71,
        Key::F3 => 0x72,
        Key::F4 => 0x73,
        Key::F5 => 0x74,
        Key::F6 => 0x75,
        Key::F7 => 0x76,
        Key::F8 => 0x77,
        Key::F9 => 0x78,
        Key::F10 => 0x79,
        Key::F11 => 0x7A,
        Key::F12 => 0x7B,
        Key::ShiftLeft => 0xA0,
        Key::ShiftRight => 0xA1,
        Key::ControlLeft => 0xA2,
        Key::ControlRight => 0xA3,
        Key::Alt => 0xA4,
        Key::AltGr => 0xA5,
        Key::MetaLeft => 0x5B,
        Key::MetaRight => 0x5C,
        _ => 0,
    }
}
