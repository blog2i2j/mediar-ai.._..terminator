const {
    Desktop,
    ElementNotFoundError,
    PlatformError,
    Selector
} = require('.');
const fs = require('fs');

async function main() {
    console.log("Starting script to find the 'Cursor' application and a button inside it.");

    const desktop = new Desktop();

    try {
        // --- List all applications ---
        console.log("\n--- Listing Running Applications ---");
        const apps = desktop.applications();
        const appNames = apps.map(app => app.name());
        console.log("Currently running applications:", appNames);

        const appName = "Cursor";
        // const appName = "Google Chrome";
        // --- Find the application ---
        console.log(`\n--- Finding '${appName}' Application ---`);
        const app = desktop.application(appName);
        console.log("Found application object:");
        console.log(app);

        // --- Retrieve and print the entire window tree using getWindowTree ---
        console.log(`\n--- Listing windows for '${appName}' application ---`);
        const windows = await desktop.windowsForApplication(appName);
        console.log(`Found ${windows.length} window(s) for ${appName}.`);
        windows.forEach((win, idx) => {
            const attrs = win.attributes();
            const isMain = attrs.properties.AXMain === "true";
            console.log(`Window #${idx + 1}:`);
            console.log("  Name:", attrs.name || attrs.label);
            console.log("  Is Main:", isMain);
        });

        // Get the PID of the application
        const pid = app.processId();
        console.log(`${appName} PID: ${pid}`);
        
        // Optional: you can provide a window title filter (null means no filter)
        const selectedWindow = windows[0];
        const windowAttrs = selectedWindow.attributes();
        const windowId = selectedWindow.id();
        const windowTitle = windowAttrs.name || windowAttrs.label;
        console.log(`Window title: ${windowTitle} (id: ${windowId})`);

        const windowTree = desktop.getWindowTree(pid, windowTitle, null);

        const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
        const fileName = `data/window_tree_${pid}_${timestamp}.json`;
        fs.writeFileSync(fileName, JSON.stringify(windowTree, null, 2));
        console.log(`\nWindow tree written to ${fileName}`);
        
        // process.exit(0);

        // --- Find a button within "Cursor" using locator('role:button').first() ---
        console.log("\n--- Finding a Button in app ---");
        try {
            // You can tweak these locators to test various scenarios
            // const locator = app.locator('role:button');
            
            const locator1 = app.locator('button:Go Back (⌃-)');
            console.log("Locator 1:", locator1);
            const locator2 = app.locator(Selector.role('button', 'Go Back (⌃-)'));
            console.log("Locator 2:", locator2);

            const locator = locator2;

            // const locator = app.locator('button:Start Debugging (F5), use (⌥F1) for accessibility help');
            // const locator = app.locator('PopUpButton:Extensions');

            console.log("Locator for button:", locator);
            const button = await locator.first();
            console.log("Found button object:");
            console.log(button);

            // Find and log all buttons in the app
            // const allButtonsLocator = app.locator('role:button');
            const allButtonsLocator = app.locator(Selector.role('button'));
            const buttons = await allButtonsLocator.all();
            console.log(`Found ${buttons.length} button(s) in the Cursor application:`);
            buttons.forEach((button, idx) => {
                const attrs = button.attributes();
                console.log(`Button #${idx + 1}:`);
                console.log("  Name:", attrs.name);
                console.log("  Role:", attrs.role);
                if (attrs.value !== undefined) {
                    console.log("  Value:", attrs.value);
                }
                if (attrs.description !== undefined) {
                    console.log("  Description:", attrs.description);
                }
            });

        } catch (e) {
            if (e instanceof ElementNotFoundError) {
                console.log("No button found in Cursor application.");
            } else {
                throw e; // Rethrow for outer catch to handle
            }
        }

        console.log("\nScript finished successfully.");

    } catch (e) {
        if (e instanceof ElementNotFoundError) {
            // This could be because no button was found in Cursor.
            console.error("\nError: Could not find the requested element.", e.message);
        } else if (e instanceof PlatformError) {
            // This is likely because the "Cursor" application isn't running.
            console.error("\nError: A platform error occurred. Is 'Cursor' running?", e.message);
        } else {
            console.error('\nAn unexpected error occurred:', e);
        }
    }
}

main().catch(err => {
    console.error('\nFatal error:', err);
}); 