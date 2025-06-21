//! Google workflow integration tests
//!
//! Tests the low-level Terminator functionality using hardcoded element IDs
//! captured from a real Google search workflow to verify if the MCP issues
//! are in the MCP layer or the underlying automation.

use crate::{AutomationError, Desktop, Selector};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

/// Test data structure containing hardcoded IDs from our Google workflow
struct GoogleWorkflowElements {
    // Edge browser window elements
    edge_window_name: &'static str,
    address_bar_id: &'static str,

    // Google search page elements
    search_combobox_id: &'static str,
    search_button_id: &'static str,
    search_by_voice_id: &'static str,
    search_by_image_id: &'static str,

    // Google color picker elements (from search results)
    color_picker_slider_id: &'static str,
    hex_input_id: &'static str,
    rgb_input_id: &'static str,
}

impl GoogleWorkflowElements {
    fn new() -> Self {
        Self {
            edge_window_name: "Google - Personal - Microsoft​ Edge",
            address_bar_id: "#3591793419895491146",

            // Initial Google page elements
            search_combobox_id: "#10480471811982966841",
            search_button_id: "#11451276010365325489",
            search_by_voice_id: "#15954871219262828317",
            search_by_image_id: "#292882688156682252",

            // Google color picker elements (after search)
            color_picker_slider_id: "#51695059635188831",
            hex_input_id: "#11217662936434863256",
            rgb_input_id: "#8285411032601946643",
        }
    }
}

#[tokio::test]
#[ignore] // Use cargo test -- --ignored to run this test
async fn test_google_workflow_low_level() -> Result<(), AutomationError> {
    crate::tests::init_tracing();
    info!("Starting Google workflow low-level test");

    let desktop = Desktop::new_default()?;
    let elements = GoogleWorkflowElements::new();

    // Step 1: Find Edge browser window
    info!("Step 1: Looking for Edge browser window");
    let applications = desktop.applications()?;

    let edge_app = applications
        .iter()
        .find(|app| {
            let name = app.name().unwrap_or_default();
            println!("Checking app: '{}'", name);
            name.contains("msedge") || name.contains("Microsoft Edge") || name.contains("Edge")
        })
        .ok_or_else(|| {
            AutomationError::ElementNotFound(
                "Edge browser not found. Please open Edge first.".to_string(),
            )
        })?;

    info!(
        "Found Edge application: {}",
        edge_app.name().unwrap_or_default()
    );

    // Step 2: Get the window tree to verify our hardcoded IDs exist
    info!("Step 2: Getting window tree to verify element IDs");
    let pid = edge_app.process_id().unwrap_or(0);
    let tree = desktop.get_window_tree(pid, None, None)?;

    // Helper function to search for elements by ID in the tree
    fn find_element_by_id(node: &crate::UINode, target_id: &str) -> bool {
        if let Some(id) = &node.id {
            if id == target_id {
                return true;
            }
        }
        node.children
            .iter()
            .any(|child| find_element_by_id(child, target_id))
    }

    // Verify critical elements exist
    let address_bar_exists = find_element_by_id(&tree, elements.address_bar_id);
    let search_box_exists = find_element_by_id(&tree, elements.search_combobox_id);

    info!("Address bar exists: {}", address_bar_exists);
    info!("Search box exists: {}", search_box_exists);

    // Step 3: Test address bar interaction
    if address_bar_exists {
        info!("Step 3: Testing address bar interaction");
        let address_locator = desktop.locator(Selector::Id(elements.address_bar_id.to_string()));

        // Get the element first, then interact with it
        match address_locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => {
                info!("✅ Address bar element found");

                match element.click() {
                    Ok(_) => {
                        info!("✅ Address bar click succeeded");
                        sleep(Duration::from_millis(500)).await;

                        // Test typing in address bar
                        match element.type_text("google.com", false) {
                            Ok(_) => info!("✅ Address bar typing succeeded"),
                            Err(e) => warn!("❌ Address bar typing failed: {}", e),
                        }
                    }
                    Err(e) => warn!("❌ Address bar click failed: {}", e),
                }
            }
            Err(e) => warn!("❌ Address bar element not found: {}", e),
        }
    } else {
        warn!(
            "❌ Address bar not found with ID: {}",
            elements.address_bar_id
        );
    }

    // Step 4: Test search box interaction (if on Google page)
    if search_box_exists {
        info!("Step 4: Testing search box interaction");
        let search_locator = desktop.locator(Selector::Id(elements.search_combobox_id.to_string()));

        match search_locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => {
                info!("✅ Search box element found");

                match element.click() {
                    Ok(_) => {
                        info!("✅ Search box click succeeded");
                        sleep(Duration::from_millis(500)).await;

                        // Test typing in search box
                        match element.type_text("terminator automation framework", false) {
                            Ok(_) => info!("✅ Search box typing succeeded"),
                            Err(e) => warn!("❌ Search box typing failed: {}", e),
                        }
                    }
                    Err(e) => warn!("❌ Search box click failed: {}", e),
                }
            }
            Err(e) => warn!("❌ Search box element not found: {}", e),
        }
    } else {
        info!("Search box not found (might not be on Google page yet)");
    }

    // Step 5: Test search button interaction
    let search_button_locator =
        desktop.locator(Selector::Id(elements.search_button_id.to_string()));
    match search_button_locator
        .first(Some(Duration::from_secs(3)))
        .await
    {
        Ok(element) => {
            match element.click() {
                Ok(_) => {
                    info!("✅ Search button click succeeded");
                    sleep(Duration::from_secs(2)).await; // Wait for search results
                }
                Err(e) => warn!("❌ Search button click failed: {}", e),
            }
        }
        Err(e) => warn!("❌ Search button element not found: {}", e),
    }

    // Step 6: After search, check for color picker elements
    info!("Step 6: Checking for color picker elements after search");
    let updated_tree = desktop.get_window_tree(pid, None, None)?;

    let color_picker_exists = find_element_by_id(&updated_tree, elements.color_picker_slider_id);
    let hex_input_exists = find_element_by_id(&updated_tree, elements.hex_input_id);

    info!("Color picker slider exists: {}", color_picker_exists);
    info!("HEX input exists: {}", hex_input_exists);

    if color_picker_exists {
        info!("Step 7: Testing color picker interaction");
        let slider_locator =
            desktop.locator(Selector::Id(elements.color_picker_slider_id.to_string()));

        match slider_locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => match element.click() {
                Ok(_) => info!("✅ Color picker slider click succeeded"),
                Err(e) => warn!("❌ Color picker slider click failed: {}", e),
            },
            Err(e) => warn!("❌ Color picker slider element not found: {}", e),
        }
    }

    if hex_input_exists {
        info!("Step 8: Testing HEX input interaction");
        let hex_locator = desktop.locator(Selector::Id(elements.hex_input_id.to_string()));

        match hex_locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => match element.click() {
                Ok(_) => {
                    info!("✅ HEX input click succeeded");
                    sleep(Duration::from_millis(500)).await;

                    match element.type_text("#FF5733", false) {
                        Ok(_) => info!("✅ HEX input typing succeeded"),
                        Err(e) => warn!("❌ HEX input typing failed: {}", e),
                    }
                }
                Err(e) => warn!("❌ HEX input click failed: {}", e),
            },
            Err(e) => warn!("❌ HEX input element not found: {}", e),
        }
    }

    info!("Google workflow low-level test completed");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_element_timing_and_responsiveness() -> Result<(), AutomationError> {
    crate::tests::init_tracing();
    info!("Starting element timing and responsiveness test");

    let desktop = Desktop::new_default()?;
    let elements = GoogleWorkflowElements::new();

    // Find Edge
    let applications = desktop.applications()?;
    let edge_app = applications
        .iter()
        .find(|app| {
            app.name().unwrap_or_default().contains("msedge")
                || app.name().unwrap_or_default().contains("Microsoft Edge")
        })
        .ok_or_else(|| AutomationError::ElementNotFound("Edge browser not found".to_string()))?;

    let pid = edge_app.process_id().unwrap_or(0);

    // Test timing for different operations
    let start = std::time::Instant::now();
    let _tree = desktop.get_window_tree(pid, None, None)?;
    let tree_time = start.elapsed();
    info!("get_window_tree took: {:?}", tree_time);

    // Test element location timing
    let start = std::time::Instant::now();
    let address_locator = desktop.locator(Selector::Id(elements.address_bar_id.to_string()));
    let _element = address_locator.first(Some(Duration::from_secs(3))).await;
    let locate_time = start.elapsed();
    info!("Element location took: {:?}", locate_time);

    // Test click timing with different timeouts
    let timeouts = [1000, 3000, 5000]; // milliseconds

    for timeout in timeouts {
        info!("Testing click with {}ms timeout", timeout);
        let start = std::time::Instant::now();

        // Create a new locator for each test to avoid state issues
        let test_locator = desktop.locator(Selector::Id(elements.address_bar_id.to_string()));

        match tokio::time::timeout(
            Duration::from_millis(timeout),
            test_locator.first(Some(Duration::from_millis(timeout))),
        )
        .await
        {
            Ok(Ok(element)) => match element.click() {
                Ok(_) => {
                    let click_time = start.elapsed();
                    info!(
                        "✅ Click succeeded in {:?} (timeout: {}ms)",
                        click_time, timeout
                    );
                }
                Err(e) => {
                    let click_time = start.elapsed();
                    warn!(
                        "❌ Click failed in {:?} (timeout: {}ms): {}",
                        click_time, timeout, e
                    );
                }
            },
            Ok(Err(e)) => {
                let find_time = start.elapsed();
                warn!(
                    "❌ Element find failed in {:?} (timeout: {}ms): {}",
                    find_time, timeout, e
                );
            }
            Err(_) => {
                warn!("❌ Operation timed out after {}ms", timeout);
            }
        }

        sleep(Duration::from_millis(1000)).await; // Wait between tests
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_selector_variations() -> Result<(), AutomationError> {
    crate::tests::init_tracing();
    info!("Starting selector variations test");

    let desktop = Desktop::new_default()?;

    // Find Edge
    let applications = desktop.applications()?;
    let _edge_app = applications
        .iter()
        .find(|app| {
            app.name().unwrap_or_default().contains("msedge")
                || app.name().unwrap_or_default().contains("Microsoft Edge")
        })
        .ok_or_else(|| AutomationError::ElementNotFound("Edge browser not found".to_string()))?;

    // Test different selector types for the same element
    let selectors = vec![
        ("ID", Selector::Id("#3591793419895491146".to_string())),
        ("Name", Selector::Name("Address and search bar".to_string())),
        (
            "Role",
            Selector::Role {
                role: "Edit".to_string(),
                name: None,
            },
        ),
    ];

    for (selector_type, selector) in selectors {
        info!("Testing {} selector: {:?}", selector_type, selector);

        let locator = desktop.locator(selector);
        let start = std::time::Instant::now();

        match locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => {
                let find_time = start.elapsed();
                info!(
                    "✅ {} selector found element in {:?}: {}",
                    selector_type,
                    find_time,
                    element.name().unwrap_or_default()
                );
            }
            Err(e) => {
                let find_time = start.elapsed();
                warn!(
                    "❌ {} selector failed in {:?}: {}",
                    selector_type, find_time, e
                );
            }
        }
    }

    Ok(())
}

/// Test to simulate the exact MCP workflow step by step
#[tokio::test]
#[ignore]
async fn test_mcp_workflow_simulation() -> Result<(), AutomationError> {
    crate::tests::init_tracing();
    info!("Starting MCP workflow simulation test");

    let desktop = Desktop::new_default()?;

    // Step 1: Open Edge (simulate run_command "start msedge")
    info!("Step 1: Simulating Edge opening");
    let _output = desktop.run_command(Some("start msedge"), None).await?;
    sleep(Duration::from_secs(3)).await; // Wait for Edge to start

    // Step 2: Get applications (simulate get_applications)
    info!("Step 2: Getting applications list");
    let applications = desktop.applications()?;
    let edge_apps: Vec<_> = applications
        .iter()
        .filter(|app| {
            let name = app.name().unwrap_or_default();
            name.contains("msedge") || name.contains("Microsoft Edge")
        })
        .collect();

    info!("Found {} Edge processes", edge_apps.len());
    for (i, app) in edge_apps.iter().enumerate() {
        info!(
            "  Edge app {}: {} (PID: {})",
            i,
            app.name().unwrap_or_default(),
            app.process_id().unwrap_or(0)
        );
    }

    if let Some(edge_app) = edge_apps.first() {
        let pid = edge_app.process_id().unwrap_or(0);

        // Step 3: Get window tree (simulate get_window_tree)
        info!("Step 3: Getting window tree for PID {}", pid);
        let tree = desktop.get_window_tree(pid, None, None)?;
        info!("Window tree has {} top-level children", tree.children.len());

        // Step 4: Simulate activate_element (window activation)
        info!("Step 4: Activating Edge window");
        match edge_app.activate_window() {
            Ok(_) => info!("✅ Window activation succeeded"),
            Err(e) => warn!("❌ Window activation failed: {}", e),
        }

        sleep(Duration::from_secs(1)).await;

        // Step 5: Simulate press_key_global {Ctrl}l (address bar focus)
        info!("Step 5: Focusing address bar with Ctrl+L");
        // Note: This would require implementing key press simulation in Desktop
        // For now, we'll test direct element interaction

        // Step 6: Simulate typing "google.com"
        info!("Step 6: Testing address bar interaction");
        let address_locator = desktop.locator(Selector::Id("#3591793419895491146".to_string()));

        match address_locator.first(Some(Duration::from_secs(3))).await {
            Ok(element) => match element.click() {
                Ok(_) => {
                    info!("✅ Address bar click succeeded");

                    match element.type_text("google.com", false) {
                        Ok(_) => info!("✅ Typing 'google.com' succeeded"),
                        Err(e) => warn!("❌ Typing failed: {}", e),
                    }
                }
                Err(e) => warn!("❌ Address bar click failed: {}", e),
            },
            Err(e) => warn!("❌ Address bar element not found: {}", e),
        }

        // Step 7: Simulate Enter key press
        info!("Step 7: Simulating Enter key press");
        // This would require key press implementation

        // Wait for page load
        sleep(Duration::from_secs(3)).await;

        // Step 8: Get updated window tree
        info!("Step 8: Getting updated window tree after navigation");
        let updated_tree = desktop.get_window_tree(pid, None, None)?;

        // Check if we can find Google search elements
        fn search_for_google_elements(node: &crate::UINode) -> Vec<String> {
            let mut found_elements = Vec::new();

            if let Some(id) = &node.id {
                if id.contains("search") || id.contains("Search") {
                    found_elements.push(format!(
                        "ID: {} ({})",
                        id,
                        node.attributes.name.as_deref().unwrap_or("No name")
                    ));
                }
            }

            if let Some(name) = &node.attributes.name {
                if name.to_lowercase().contains("search") {
                    found_elements
                        .push(format!("Name: {} (Role: {})", name, &node.attributes.role));
                }
            }

            for child in &node.children {
                found_elements.extend(search_for_google_elements(child));
            }

            found_elements
        }

        let google_elements = search_for_google_elements(&updated_tree);
        info!(
            "Found {} potential Google search elements:",
            google_elements.len()
        );
        for element in google_elements.iter().take(10) {
            // Limit output
            info!("  {}", element);
        }
    }

    info!("MCP workflow simulation completed");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_debug_available_windows() -> Result<(), AutomationError> {
    println!("Debug: Listing all available windows");
    let desktop = Desktop::new_default()?;

    match desktop.applications() {
        Ok(apps) => {
            println!("Found {} applications:", apps.len());
            for app in apps {
                println!(
                    "App: {} (ID: {:?})",
                    app.name().unwrap_or_default(),
                    app.id()
                );

                // Try to get windows for this app
                match desktop
                    .windows_for_application(&app.name().unwrap_or_default())
                    .await
                {
                    Ok(windows) => {
                        for window in windows {
                            println!(
                                "  Window: {} (ID: {:?})",
                                window.name().unwrap_or_default(),
                                window.id()
                            );
                        }
                    }
                    Err(e) => println!("  Error getting windows: {}", e),
                }
            }
        }
        Err(e) => println!("Error getting applications: {}", e),
    }

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_live_element_interaction() -> Result<(), AutomationError> {
    println!("Testing live element interaction with current page elements");
    let desktop = Desktop::new_default()?;

    // Find Edge
    let applications = desktop.applications()?;
    let edge_app = applications
        .iter()
        .find(|app| {
            let name = app.name().unwrap_or_default();
            name.contains("Microsoft​ Edge") || name.contains("Edge")
        })
        .ok_or_else(|| AutomationError::ElementNotFound("Edge browser not found".to_string()))?;

    println!("Found Edge: {}", edge_app.name().unwrap_or_default());

    // Try to find address bar by name instead of ID
    let address_locator = desktop.locator(Selector::Name("Address and search bar".to_string()));

    println!("Looking for address bar by name...");
    match address_locator.first(Some(Duration::from_secs(5))).await {
        Ok(element) => {
            println!("✅ Found address bar by name!");
            println!("Element ID: {:?}", element.id());
            println!("Element name: {:?}", element.name());

            // Test clicking
            println!("Testing click...");
            match element.click() {
                Ok(_) => {
                    println!("✅ Address bar click SUCCESS!");

                    // Test typing
                    sleep(Duration::from_millis(500)).await;
                    match element.type_text("example.com", false) {
                        Ok(_) => println!("✅ Typing SUCCESS!"),
                        Err(e) => println!("❌ Typing failed: {}", e),
                    }
                }
                Err(e) => println!("❌ Click failed: {}", e),
            }
        }
        Err(e) => {
            println!("❌ Address bar not found by name: {}", e);

            // Try by role
            println!("Trying to find address bar by role...");
            let role_locator = desktop.locator(Selector::Role {
                role: "Edit".to_string(),
                name: None,
            });

            match role_locator.first(Some(Duration::from_secs(3))).await {
                Ok(element) => {
                    println!("✅ Found address bar by role!");
                    println!("Element name: {:?}", element.name());

                    match element.click() {
                        Ok(_) => println!("✅ Role-based click SUCCESS!"),
                        Err(e) => println!("❌ Role-based click failed: {}", e),
                    }
                }
                Err(e) => println!("❌ Address bar not found by role either: {}", e),
            }
        }
    }

    Ok(())
}
