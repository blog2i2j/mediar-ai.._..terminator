# Terminator CLI

The Terminator CLI is a powerful command-line tool for managing the Terminator project, including version management, releases, **Azure VM deployment**, and **MCP server interaction**.

## Features

- 📦 **Version Management**: Bump and sync versions across all packages
- 🏷️ **Release Automation**: Tag and release with a single command
- ☁️ **Azure VM Deployment**: One-liner to deploy Windows VMs with MCP server
- 🤖 **MCP Client**: Chat with MCP servers over HTTP or stdio
- 🔄 **Workflow Execution**: Run automation workflows from YAML/JSON files
- 🔧 **Tool Execution**: Execute individual MCP tools directly
- 🔒 **Secure by Default**: Auto-generated passwords, configurable security rules

## Installation

From the workspace root:

```bash
# Build the CLI
cargo build --release --bin terminator

# Install globally (optional)
cargo install --path terminator-cli
```

## Quick Start

### Version Management

```bash
# Bump version
terminator patch      # x.y.Z+1
terminator minor      # x.Y+1.0
terminator major      # X+1.0.0

# Sync all package versions
terminator sync

# Show current status
terminator status

# Tag and push
terminator tag

# Full release (bump + tag + push)
terminator release        # patch release
terminator release minor  # minor release
```

### MCP Workflow Execution

Execute automation workflows from YAML or JSON files:

```bash
# Execute a workflow file
terminator mcp run workflow.yml

# Execute with verbose logging
terminator mcp run workflow.yml --verbose

# Dry run (validate without executing)
terminator mcp run workflow.yml --dry-run

# Use specific MCP server command
terminator mcp run workflow.yml --command "npx -y terminator-mcp-agent@latest"

# Use HTTP MCP server
terminator mcp run workflow.yml --url http://localhost:3000/mcp

# Pass input values to workflow (available as env variables in scripts)
terminator mcp run workflow.yml --inputs '{"username":"john","api_key":"abc123"}'

# Combine inputs with other options
terminator mcp run workflow.yml --inputs '{"count":5}' --verbose
```

### Passing Input Values to Workflows

The `--inputs` parameter allows you to pass initial values to your workflow that can be accessed by JavaScript/Python scripts:

```bash
# Pass inputs as JSON
terminator mcp run workflow.yml --inputs '{"user":"alice","count":42,"enabled":true}'
```

These inputs are accessible in your workflow scripts:
- In JavaScript: via `env.inputs` object or directly as `env.username`, `env.count`, etc.
- In Python: via `env` dictionary
- Inputs override any default values defined in the workflow file

**Example workflow file** (`workflow.yml`):
```yaml
tool_name: execute_sequence
arguments:
  steps:
    - tool_name: navigate_browser
      arguments:
        url: "https://example.com"
    - tool_name: click_element
      arguments:
        selector: "role:Button|name:Submit"
    - tool_name: get_focused_window_tree
      id: capture_result
  output_parser:
    ui_tree_source_step_id: capture_result
    javascript_code: |
      // Extract all checkbox names
      const results = [];
      
      function findElementsRecursively(element) {
          if (element.attributes && element.attributes.role === 'CheckBox') {
              const item = {
                  name: element.attributes.name || ''
              };
              results.push(item);
          }
          
          if (element.children) {
              for (const child of element.children) {
                  findElementsRecursively(child);
              }
          }
      }
      
      findElementsRecursively(tree);
      return results;
```

**JavaScript execution in workflows**:
```yaml
tool_name: execute_sequence
arguments:
  steps:
    - tool_name: run_command
      arguments:
        engine: "javascript"
        run: |
          // Access inputs passed from CLI via --inputs parameter
          console.log(`Processing for user: ${env.username}`);
          console.log(`Count value: ${env.count}`);

          // Or access the entire inputs object
          const allInputs = env.inputs;
          console.log(`All inputs:`, JSON.stringify(allInputs));

          // Use inputs in your logic
          for (let i = 0; i < env.count; i++) {
            console.log(`Processing item ${i + 1} for ${env.username}`);
          }

          return {
            processed_by: env.username,
            items_processed: env.count
          };
```

**Original example**:
```yaml
tool_name: execute_sequence
arguments:
  steps:
    - tool_name: run_command
      arguments:
        engine: "node"
        script: |
          // Access desktop automation APIs
          const elements = await desktop.locator('role:button').all();
          log(`Found ${elements.length} buttons`);
          
          // Interact with UI elements
          for (const element of elements) {
            const name = await element.name();
            if (name.includes('Submit')) {
              await element.click();
              break;
            }
          }
          
          return {
            buttons_found: elements.length,
            action: 'clicked_submit'
          };
```

### MCP Tool Execution

Execute individual MCP tools directly:

```bash
# Execute a single tool
terminator mcp exec get_applications

# Execute with arguments
terminator mcp exec click_element '{"selector": "role:Button|name:OK"}'

# Use different MCP server
terminator mcp exec --url http://localhost:3000/mcp validate_element '{"selector": "#button"}'
```

### Interactive MCP Chat

Chat with MCP servers interactively:

```bash
# Start chat session (uses local MCP server by default)
terminator mcp chat

# Chat with remote MCP server
terminator mcp chat --url https://your-server.com/mcp

# Chat with specific MCP server command
terminator mcp chat --command "node my-mcp-server.js"
```

### Control Remote Computer Through Chat

1. Run the MCP server on your remote machine
2. Open port or use ngrok
3. Connect via CLI:

```bash
terminator mcp chat --url https://xxx/mcp
```

<img width="1512" alt="Screenshot 2025-07-04 at 1 49 10 PM" src="https://github.com/user-attachments/assets/95355099-0130-4702-bd11-0278db181253" />

## Advanced Usage

### MCP Server Connection Options

The CLI supports multiple ways to connect to MCP servers:

```bash
# Local MCP server (default - uses @latest for compatibility)
terminator mcp run workflow.yml

# Specific version
terminator mcp run workflow.yml --command "npx -y terminator-mcp-agent@0.9.0"

# HTTP server
terminator mcp run workflow.yml --url http://localhost:3000/mcp

# Custom server command
terminator mcp run workflow.yml --command "python my_mcp_server.py"
```

### Workflow File Formats

The CLI supports both YAML and JSON workflow files:

**Direct workflow (workflow.yml)**:
```yaml
steps:
  - tool_name: navigate_browser
    arguments:
      url: "https://example.com"
stop_on_error: true
```

**Tool call wrapper (workflow.json)**:
```json
{
  "tool_name": "execute_sequence",
  "arguments": {
    "steps": [
      {
        "tool_name": "navigate_browser",
        "arguments": {
          "url": "https://example.com"
        }
      }
    ]
  }
}
```

### Error Handling

```bash
# Continue on errors
terminator mcp run workflow.yml --continue-on-error

# Custom timeout
terminator mcp run workflow.yml --timeout 30000

# Detailed error output
terminator mcp run workflow.yml --verbose
```

### Code Execution in Workflows

The CLI supports executing code within workflows using the `run_command` tool in engine mode, providing access to desktop automation APIs:

**Available Engines:**
- `nodejs` - Full Node.js runtime with desktop APIs
- `quickjs` - Lightweight JavaScript engine (default)

**Desktop APIs Available:**
```javascript
// Element discovery
const elements = await desktop.locator('role:button|name:Submit').all();
const element = await desktop.locator('#button-id').first();

// Element interaction
await element.click();
await element.type('Hello World');
await element.setToggled(true);

// Property access
const name = await element.name();
const bounds = await element.bounds();
const isEnabled = await element.enabled();

// Utilities
log('Debug message');  // Logging
await sleep(1000);     // Delay in milliseconds
```

**Example Use Cases:**
```yaml
# Conditional logic based on UI state
- tool_name: run_command
  arguments:
    engine: "node"
    script: |
      const submitButton = await desktop.locator('role:button|name:Submit').first();
      const isEnabled = await submitButton.enabled();
      
      if (isEnabled) {
        await submitButton.click();
        return { action: 'submitted' };
      } else {
        log('Submit button is disabled, checking form validation...');
        return { action: 'validation_needed' };
      }

# Bulk operations on multiple elements
- tool_name: run_command
  arguments:
    engine: "javascript"
    script: |
      const checkboxes = await desktop.locator('role:checkbox').all();
      let enabledCount = 0;
      
      for (const checkbox of checkboxes) {
        await checkbox.setToggled(true);
        enabledCount++;
        await sleep(50); // Small delay between operations
      }
      
      return { total_enabled: enabledCount };

# Dynamic element discovery and interaction
- tool_name: run_command
  arguments:
    engine: "javascript"
    script: |
      // Find all buttons containing specific text
      const buttons = await desktop.locator('role:button').all();
      const targets = [];
      
      for (const button of buttons) {
        const name = await button.name();
        if (name.toLowerCase().includes('download')) {
          targets.push(name);
          await button.click();
          await sleep(1000);
        }
      }
      
      return { downloaded_items: targets };
```

## Configuration

### Environment Variables

- `RUST_LOG`: Set logging level (e.g., `debug`, `info`, `warn`, `error`)
- `MCP_SERVER_URL`: Default MCP server URL
- `MCP_SERVER_COMMAND`: Default MCP server command

### Default Behavior

- **MCP Server**: Uses `npx -y terminator-mcp-agent@latest` by default
- **Logging**: Info level by default, debug with `--verbose`
- **Error Handling**: Stops on first error by default
- **Format**: Auto-detects YAML/JSON from file extension

## Troubleshooting

### Version Mismatch Issues

If you encounter "missing field" errors, ensure you're using the latest MCP server:

```bash
# Force latest version
terminator mcp run workflow.yml --command "npx -y terminator-mcp-agent@latest"

# Clear npm cache if needed
npm cache clean --force
```

### Connection Issues

```bash
# Test MCP server connectivity
terminator mcp exec get_applications

# Use verbose logging for debugging
terminator mcp run workflow.yml --verbose

# Test with dry run first
terminator mcp run workflow.yml --dry-run
```

### JavaScript Execution Issues

```bash
# Test JavaScript execution capability via run_command (engine mode)
terminator mcp exec run_command '{"engine": "javascript", "run": "return {test: true};"}'

# Use node engine for full APIs
terminator mcp exec run_command '{"engine": "node", "run": "const elements = await desktop.locator(\"role:button\").all(); return {count: elements.length};"}'

# Run Python with terminator.py
terminator mcp exec run_command '{"engine": "python", "run": "return {\"ok\": True}"}'

# Debug JavaScript errors with verbose logging
terminator mcp run workflow.yml --verbose
```

For more examples and advanced usage, see the [Terminator MCP Agent documentation](../terminator-mcp-agent/README.md).