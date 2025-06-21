const {
    Desktop,
    ElementNotFoundError,
    PlatformError
} = require('.');
const fs = require('fs');
const path = require('path');

async function main() {
    const appName = process.argv[2];

    if (!appName) {
        console.error("Usage: node dump_app_tree.js <ApplicationName>");
        process.exit(1);
    }

    console.log(`Starting script to dump UI tree for '${appName}'.`);

    const desktop = new Desktop();

    try {
        // Find the application
        console.log(`\n--- Finding '${appName}' Application ---`);
        const app = desktop.application(appName);
        console.log("Found application object.");

        // Get the PID
        const pid = app.processId();
        console.log(`'${appName}' PID: ${pid}`);

        // Retrieve the window tree
        console.log("\n--- Retrieving UI tree ---");
        const windowTree = desktop.getWindowTree(pid, null, null);

        // Generate file names
        const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
        const safeAppName = appName.replace(/[^a-z0-9]/gi, '_').toLowerCase();
        const baseFileName = `data/${safeAppName}_${pid}_${timestamp}`;
        const jsonFileName = `${baseFileName}.json`;
        const mdFileName = `${baseFileName}.md`;

        // Write to .json file
        fs.writeFileSync(jsonFileName, JSON.stringify(windowTree, null, 2));
        console.log(`\nUI tree written to ${jsonFileName}`);

        // Write to .md file (stub)
        fs.writeFileSync(mdFileName, "```json\n" + JSON.stringify(windowTree, null, 2) + "\n```");
        console.log(`UI tree also written to ${mdFileName}`);


        console.log("\nScript finished successfully.");

    } catch (e) {
        if (e instanceof ElementNotFoundError) {
            console.error(`\nError: Could not find the application '${appName}'. Is it running?`);
        } else if (e instanceof PlatformError) {
            console.error(`\nError: A platform error occurred while working with '${appName}'.`, e.message);
        } else {
            console.error('\nAn unexpected error occurred:', e);
        }
        process.exit(1);
    }
}

main().catch(err => {
    console.error('\nFatal error:', err);
    process.exit(1);
}); 