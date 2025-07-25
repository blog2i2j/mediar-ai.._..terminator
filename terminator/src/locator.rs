use tracing::{debug, instrument};

use crate::element::UIElement;
use crate::errors::AutomationError;
use crate::platforms::AccessibilityEngine;
use crate::selector::Selector;
use std::sync::Arc;
use std::time::Duration;
use tokio::task;

// Default timeout if none is specified on the locator itself
const DEFAULT_LOCATOR_TIMEOUT: Duration = Duration::from_secs(30);

/// A high-level API for finding and interacting with UI elements
///
/// For maximum precision, prefer role|name format (e.g., "button|Submit")
/// over broad selectors like "role:Button" that could match multiple elements.
#[derive(Clone)]
pub struct Locator {
    engine: Arc<dyn AccessibilityEngine>,
    selector: Selector,
    timeout: Duration, // Default timeout for this locator instance
    root: Option<UIElement>,
}

impl Locator {
    /// Create a new locator with the given selector
    pub(crate) fn new(engine: Arc<dyn AccessibilityEngine>, selector: Selector) -> Self {
        Self {
            engine,
            selector,
            timeout: DEFAULT_LOCATOR_TIMEOUT, // Use default
            root: None,
        }
    }

    /// Set a default timeout for waiting operations on this locator instance.
    /// This timeout is used if no specific timeout is passed to action/wait methods.
    pub fn set_default_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the root element for this locator
    pub fn within(mut self, element: UIElement) -> Self {
        self.root = Some(element);
        self
    }

    /// Get all elements matching this locator, waiting up to the specified timeout.
    /// If no timeout is provided, uses the locator's default timeout.
    pub async fn all(
        &self,
        timeout: Option<Duration>,
        depth: Option<usize>,
    ) -> Result<Vec<UIElement>, AutomationError> {
        let effective_timeout = timeout.unwrap_or(self.timeout);
        // find_elements itself handles the timeout now
        self.engine.find_elements(
            &self.selector,
            self.root.as_ref(),
            Some(effective_timeout),
            depth,
        )
    }

    pub async fn first(&self, timeout: Option<Duration>) -> Result<UIElement, AutomationError> {
        let element = self.wait(timeout).await?;
        Ok(element)
    }

    /// Wait for an element matching the locator to appear, up to the specified timeout.
    /// If no timeout is provided, uses the locator's default timeout.
    #[instrument(level = "debug", skip(self, timeout))]
    pub async fn wait(&self, timeout: Option<Duration>) -> Result<UIElement, AutomationError> {
        debug!("Waiting for element matching selector: {:?}", self.selector);

        if let Selector::Invalid(reason) = &self.selector {
            return Err(AutomationError::InvalidSelector(reason.clone()));
        }

        let effective_timeout = timeout.unwrap_or(self.timeout);

        // Since the underlying engine's find_element is a blocking call that
        // already handles polling and timeouts, we should not wrap it in another async loop.
        // Instead, we run it in a blocking-safe thread to avoid stalling the async runtime.
        let engine = self.engine.clone();
        let selector = self.selector.clone();
        let root = self.root.clone();

        task::spawn_blocking(move || {
            engine.find_element(&selector, root.as_ref(), Some(effective_timeout))
        })
        .await
        .map_err(|e| AutomationError::PlatformError(format!("Task join error: {e}")))?
        .map_err(|e| {
            // The engine returns ElementNotFound on timeout. We convert it to a more specific Timeout error here.
            if let AutomationError::ElementNotFound(inner_msg) = e {
                AutomationError::Timeout(format!(
                    "Timed out after {effective_timeout:?} waiting for element {}. Original error: {inner_msg}",
                    self.selector_string()
                ))
            } else {
                e
            }
        })
    }

    fn append_selector(&self, selector_to_append: Selector) -> Locator {
        let mut new_chain = match self.selector.clone() {
            Selector::Chain(existing_chain) => existing_chain,
            s if s != Selector::Path("/".to_string()) => vec![s], // Assuming root path is default
            _ => vec![],
        };

        // Append the new selector, flattening if it's also a chain
        match selector_to_append {
            Selector::Chain(mut next_chain_parts) => {
                new_chain.append(&mut next_chain_parts);
            }
            s => new_chain.push(s),
        }

        Locator {
            engine: self.engine.clone(),
            selector: Selector::Chain(new_chain),
            timeout: self.timeout,
            root: self.root.clone(),
        }
    }

    /// Adds a filter to find elements based on their visibility.
    pub fn visible(&self, is_visible: bool) -> Locator {
        self.append_selector(Selector::Visible(is_visible))
    }

    /// Get a nested locator
    pub fn locator(&self, selector: impl Into<Selector>) -> Locator {
        self.append_selector(selector.into())
    }

    pub fn selector_string(&self) -> String {
        format!("{:?}", self.selector)
    }
}
