//! Tests for UI element selectors

use crate::{Desktop, Selector, UIElement};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// A test fixture that manages the lifecycle of an application for testing.
/// It ensures the application is opened before the test runs and closed after,
/// even if the test panics.
struct AppFixture {
    desktop: Arc<Desktop>,
    app: Option<UIElement>,
    rt: Runtime,
}

impl AppFixture {
    /// Creates a new fixture and launches the specified application.
    fn new(app_name: &str) -> Self {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        let desktop = Arc::new(Desktop::new(false, false).expect("Failed to create Desktop"));
        let desktop_clone = desktop.clone();

        let app = rt.block_on(async {
            match desktop_clone.open_application(app_name) {
                Ok(app) => {
                    // Wait a bit for the app to be fully ready
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    Some(app)
                }
                Err(e) => {
                    panic!(
                        "Failed to open application '{}' for testing: {}",
                        app_name, e
                    );
                }
            }
        });

        AppFixture { desktop, app, rt }
    }
}

impl Drop for AppFixture {
    /// Ensures the application is closed when the fixture goes out of scope.
    fn drop(&mut self) {
        if let Some(app) = self.app.take() {
            let app_name = app.name().unwrap_or_else(|| "unknown".to_string());
            println!("--- Tearing down test, closing '{}' ---", app_name);
            if let Err(e) = app.close() {
                // It might already be closed, so just log the error.
                eprintln!(
                    "Error closing application '{}' during test teardown: {}",
                    app_name, e
                );
            }
            // Give a moment for the process to terminate
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[test]
fn test_id_selector_finds_element() {
    let fixture = AppFixture::new("notepad");
    let desktop = fixture.desktop.clone();
    let rt = &fixture.rt;

    rt.block_on(async {
        // 1. Find the main text area (the "Edit" control in Notepad) to get a target element.
        let edit_selector = Selector::Role {
            role: "edit".to_string(),
            name: None,
        };
        let text_area = desktop
            .locator(edit_selector)
            .first(None)
            .await
            .expect("Could not find the text area in Notepad.");

        // 2. Get the ID of the target element.
        let target_id = text_area.id().expect("Text area should have an ID.");
        println!("Found text area with ID: {}", target_id);
        assert!(!target_id.is_empty(), "Element ID should not be empty.");

        // 3. Create an ID selector from the retrieved ID.
        let id_selector_str = format!("#{}", target_id);
        let id_selector = Selector::from(id_selector_str.as_str());

        // 4. Use the ID selector to find the element again.
        let found_element = desktop
            .locator(id_selector)
            .first(None)
            .await
            .expect("Failed to find element using ID selector.");

        // 5. Verify that the found element is the same as the original.
        let found_id = found_element
            .id()
            .expect("Found element should have an ID.");
        assert_eq!(
            target_id, found_id,
            "The ID of the found element must match the target ID."
        );

        println!("✅ Successfully found element by ID: {}", target_id);
    });
}

#[test]
fn test_click_by_position_in_settings() {
    // Using a more specific app name for settings
    let fixture = AppFixture::new("ms-settings:");
    let desktop = fixture.desktop.clone();
    let rt = &fixture.rt;

    rt.block_on(async {
        // 1. Find the "System" button on the main settings page.
        // We give it a few seconds to appear.
        let system_button_selector = Selector::Role {
            role: "listitem".to_string(),
            name: Some("System".to_string()),
        };
        let system_button = desktop
            .locator(system_button_selector)
            .first(Some(std::time::Duration::from_secs(5)))
            .await
            .expect("Could not find the 'System' button in Settings.");

        // 2. Get its bounds and calculate the center point.
        let (x, y, width, height) = system_button
            .bounds()
            .expect("System button should have bounds.");
        let center_x = (x + width / 2.0) as i32;
        let center_y = (y + height / 2.0) as i32;
        println!(
            "Found 'System' button at ({}, {}). Clicking center at ({}, {})",
            x, y, center_x, center_y
        );

        // 3. Create a position selector for the center point.
        let pos_selector = Selector::from(format!("pos:{},{}", center_x, center_y).as_str());

        // 4. Use the position selector to find the element and click it.
        let element_at_pos = desktop
            .locator(pos_selector)
            .first(None)
            .await
            .expect("Failed to find element using position selector.");

        element_at_pos
            .click()
            .expect("Clicking by position failed.");

        // 5. Verify the click was successful by checking for navigation.
        // On the "System" page, there should be a "Display" option.
        println!("Clicked by position. Verifying navigation to System page...");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await; // Wait for page to load

        let display_selector = Selector::Name("Display".to_string());
        let display_item = desktop
            .locator(display_selector)
            .wait(Some(std::time::Duration::from_secs(5)))
            .await;

        assert!(
            display_item.is_ok(),
            "Failed to find 'Display' item after click. Navigation to System page likely failed."
        );

        println!("✅ Successfully clicked element by position and verified navigation.");

        // now find the close button and click it
        let close_button_selector = Selector::Role {
            role: "button".to_string(),
            name: Some("Close".to_string()),
        };
        let close_button = desktop
            .locator(close_button_selector)
            .first(None)
            .await
            .expect("Could not find the 'Close' button in Settings.");
        close_button
            .click()
            .expect("Clicking the 'Close' button failed.");
    });
}

#[test]
fn test_stable_id_across_sessions() {
    // Helper function to run the stability check on a given app
    fn check_id_stability(app_name: &str, selector: Selector, element_description: &str) {
        println!("\n--- Testing ID stability for {} ---", app_name);
        let fixture = AppFixture::new(app_name);
        let desktop = fixture.desktop.clone();
        let rt = &fixture.rt;

        rt.block_on(async {
            // Give the app a moment to start, especially Settings.
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            // 1. Find the element for the first time.
            let element_1 = desktop
                .locator(selector.clone())
                .first(Some(std::time::Duration::from_secs(5)))
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "Could not find the {} in {} (first attempt): {}",
                        element_description, app_name, e
                    )
                });

            let id_1 = element_1.id().expect("Element should have an ID.");
            println!(
                "Found {} in {}. First ID: {}",
                element_description, app_name, id_1
            );
            assert!(!id_1.is_empty(), "First element ID should not be empty.");

            // 2. Find the *exact same* element again to simulate a new session/query.
            let element_2 = desktop
                .locator(selector.clone())
                .first(Some(std::time::Duration::from_secs(5)))
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "Could not find the {} in {} (second attempt): {}",
                        element_description, app_name, e
                    )
                });

            let id_2 = element_2.id().expect("Element should have an ID.");
            println!(
                "Found {} in {} again. Second ID: {}",
                element_description, app_name, id_2
            );
            assert!(!id_2.is_empty(), "Second element ID should not be empty.");

            // 3. Verify that the IDs are identical.
            assert_eq!(
                id_1, id_2,
                "The ID for the {} in {} should be stable across sessions.",
                element_description, app_name
            );

            println!(
                "✅ ID for {} in {} is stable.",
                element_description, app_name
            );
        });
    }

    // Run the check for Notepad
    check_id_stability(
        "notepad",
        Selector::Role {
            role: "edit".to_string(),
            name: None,
        },
        "main text area",
    );

    // Run the check for Settings app
    check_id_stability(
        "ms-settings:",
        Selector::Role {
            role: "listitem".to_string(),
            name: Some("System".to_string()),
        },
        "'System' button",
    );
}
#[test]
fn test_web_id_stability() {
    println!("\n--- Testing Web ID stability (Rigorous) ---");
    let rt = Runtime::new().expect("Failed to create Tokio runtime");
    let desktop = Arc::new(Desktop::new(false, false).expect("Failed to create Desktop"));

    rt.block_on(async {
        let mut browser_window: Option<UIElement> = None;

        // A closure that returns a future, allowing it to be called multiple times.
        let check_url = |url: String,
                         element_selector: Selector,
                         element_description: String,
                         expected_title_part: String| {
            let desktop = desktop.clone();
            async move {
                println!("-- Checking URL: {} --", url);

                // 1. Use the correct open_url function to launch and navigate.
                let app = desktop
                    .open_url(&url, Some(crate::Browser::Edge))
                    .expect("Failed to open URL in Edge.");

                // Allow a few seconds for the browser to initialize and start navigation.
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                println!("✅ Browser opened to {}", url);

                // 2. RIGOROUS: Poll until the window title confirms navigation.
                let nav_timeout = std::time::Duration::from_secs(20);
                let start_time = std::time::Instant::now();
                let mut navigated = false;
                while start_time.elapsed() < nav_timeout {
                    if let Some(name) = app.name() {
                        if name.contains(&expected_title_part) {
                            println!("✅ Navigation confirmed: Window title is '{}'", name);
                            navigated = true;
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                assert!(
                    navigated,
                    "Navigation failed: Window title did not contain '{}' within timeout.",
                    expected_title_part
                );

                // 3. Find the target element on the page for the first time.
                let element_1 = app
                    .locator(element_selector.clone())
                    .unwrap()
                    .first(Some(std::time::Duration::from_secs(10)))
                    .await
                    .unwrap_or_else(|e| {
                        panic!(
                            "Could not find '{}' on {} (first attempt): {}",
                            element_description, url, e
                        )
                    });

                let id_1 = element_1.id().expect("Element should have an ID.");
                println!("Found '{}'. First ID: {}", element_description, id_1);

                // 4. Find the browser's 'Reload' button and click it to refresh the page.
                let reload_button = app
                    .locator(Selector::Role {
                        role: "button".to_string(),
                        name: Some("Refresh".to_string()),
                    })
                    .unwrap()
                    .first(None)
                    .await
                    .expect("Could not find the 'Reload' button in the browser.");

                reload_button
                    .click()
                    .expect("Failed to click Reload button.");
                println!("Reloaded page, waiting for content to load again...");

                // 5. RIGOROUS: Poll until the window title confirms reload.
                let reload_start_time = std::time::Instant::now();
                let mut reloaded = false;
                while reload_start_time.elapsed() < nav_timeout {
                    if let Some(name) = app.name() {
                        if name.contains(&expected_title_part) {
                            println!("✅ Reload confirmed: Window title is '{}'", name);
                            reloaded = true;
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                assert!(
                    reloaded,
                    "Page reload failed: Window title did not contain '{}' after reload.",
                    expected_title_part
                );

                // 6. Find the same element again after the reload.
                let element_2 = app
                    .locator(element_selector.clone())
                    .unwrap()
                    .first(Some(std::time::Duration::from_secs(10)))
                    .await
                    .unwrap_or_else(|e| {
                        panic!(
                            "Could not find '{}' on {} (second attempt, after reload): {}",
                            element_description, url, e
                        )
                    });

                let id_2 = element_2.id().expect("Element should have a second ID.");
                println!("Found '{}' again. Second ID: {}", element_description, id_2);

                // 7. Assert that the IDs are identical, proving stability.
                assert_eq!(
                    id_1, id_2,
                    "Web element ID for '{}' should be stable after a page reload.",
                    element_description
                );
                println!("✅ ID for '{}' is stable on {}.", element_description, url);

                // Return the app window for closing later
                app
            }
        };

        // Test Dataiku page
        browser_window = Some(
            check_url(
                "https://pages.dataiku.com/guide-to-ai-agents".to_string(),
                Selector::Name("Get Ahead With Agentic AI".to_string()),
                "Dataiku page title".to_string(),
                "Agents".to_string(), // Expected part of the window title
            )
            .await,
        );

        // wait 30 seconds
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        // Test Luma page
        browser_window = Some(
            check_url(
                "https://lu.ma/airstreet".to_string(),
                Selector::Name("Air Street".to_string()),
                "Luma event title".to_string(),
                "Air Street".to_string(), // Expected part of the window title
            )
            .await,
        );

        // --- Tearing down test ---
        if let Some(window) = browser_window {
            println!("--- Tearing down test, closing browser ---");
            window.close().expect("Failed to close browser window.");
        }
    });
}
