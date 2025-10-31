## Terminator MCP Agent

<!-- BADGES:START -->

[<img alt="Install in VS Code" src="https://img.shields.io/badge/VS_Code-VS_Code?style=flat-square&label=Install%20Server&color=0098FF">](https://insiders.vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D)
[<img alt="Install in VS Code Insiders" src="https://img.shields.io/badge/VS_Code_Insiders-VS_Code_Insiders?style=flat-square&label=Install%20Server&color=24bfa5">](https://insiders.vscode.dev/redirect?url=vscode-insiders%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D)

<!-- BADGES:END -->

A Model Context Protocol (MCP) server that provides desktop GUI automation capabilities using the [Terminator](https://github.com/mediar-ai/terminator) library. This server enables LLMs and agentic clients to interact with Windows, macOS, and Linux applications through structured accessibility APIs—no vision models or screenshots required.

## Quick Install

### Claude Code

Install with a single command:

```bash
claude mcp add terminator "npx -y terminator-mcp-agent@latest" -s user
```

### Cursor

Copy and paste this URL into your browser's address bar:

```
cursor://anysphere.cursor-deeplink/mcp/install?name=terminator-mcp-agent&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsInRlcm1pbmF0b3ItbWNwLWFnZW50Il19
```

Or install manually:

1. Open Cursor Settings (`Cmd/Ctrl + ,`)
2. Go to the MCP tab
3. Add server with command: `npx -y terminator-mcp-agent`

### HTTP Endpoints (when running with `-t http`)

- `GET /health`: Always returns 200 while the process is alive.
- `GET /status`: Busy-aware probe for load balancers. Returns JSON and appropriate status:
  - 200 when idle: `{ "busy": false, "activeRequests": 0, "maxConcurrent": 1, "lastActivity": "<ISO-8601>" }`
  - 503 when busy: `{ "busy": true, "activeRequests": 1, "maxConcurrent": 1, "lastActivity": "<ISO-8601>" }`
  - Content-Type is `application/json`.
- `POST /mcp`: MCP execution endpoint. Enforces single-request concurrency per machine by default.

Concurrency is controlled by the `MCP_MAX_CONCURRENT` environment variable (default `1`). Only accepted `POST /mcp` requests are counted toward `activeRequests`. If the server is at capacity, new `POST /mcp` requests return 503 immediately. This 503 behavior is intentional so an Azure Load Balancer probing `GET /status` can take a busy VM out of rotation and route traffic elsewhere.

### Getting Started

The easiest way to get started is to use the one-click install buttons above for your specific editor (VS Code, Cursor, etc.).

Alternatively, you can install and configure the agent from your command line.

**1. Install & Configure Automatically**
Run the following command and select your MCP client from the list:

```sh
npx -y terminator-mcp-agent@latest --add-to-app
```

**2. Manual Configuration**
If you prefer, you can add the following to your MCP client's settings file:

```json
{
  "mcpServers": {
    "terminator-mcp-agent": {
      "command": "npx",
      "args": ["-y", "terminator-mcp-agent@latest"]
    }
  }
}
```

### Command Line Interface (CLI) Execution

For automation workflows and CI/CD pipelines, you can execute workflows directly from the command line using the [Terminator CLI](../terminator-cli/README.md):

**Quick Start:**

```bash
# Execute a workflow file
terminator mcp run workflow.yml

# With verbose logging
terminator mcp run workflow.yml --verbose

# Dry run (validate without executing)
terminator mcp run workflow.yml --dry-run

# Use specific MCP server version
terminator mcp run workflow.yml --command "npx -y terminator-mcp-agent@latest"

# Run specific steps (requires step IDs in workflow)
terminator mcp run workflow.yml --start-from "step_12" --end-at "step_13"

# Run single step
terminator mcp run workflow.yml --start-from "read_json" --end-at "read_json"

# Execute jumps at end boundary (by default jumps are skipped at --end-at-step)
terminator mcp run workflow.yml --end-at "step_5" --execute-jumps-at-end
```

**Workflow File Formats:**

Direct workflow format (`workflow.yml`):

```yaml
steps:
  - tool_name: navigate_browser
    arguments:
      url: "https://example.com"
  - tool_name: click_element
    arguments:
      selector: "role:Button|name:Submit"
stop_on_error: true
include_detailed_results: true
```

With conditional jumps (`workflow_with_jumps.yml`):

```yaml
steps:
  - tool_name: validate_element
    id: check_logged_in
    arguments:
      selector: "role:button|name:Logout"
    jumps:
      - if: "check_logged_in_status == 'success'"
        to_id: main_app
        reason: "User already logged in - skipping authentication"

  - tool_name: click_element
    id: login_flow
    arguments:
      selector: "role:button|name:Login"
  # ... more login steps ...

  - tool_name: click_element
    id: main_app
    arguments:
      selector: "role:button|name:Dashboard"
```

Tool call wrapper format (`workflow.json`):

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

**Code Execution in Workflows (engine mode):**

Execute custom JavaScript or Python with access to desktop automation APIs via `run_command`.

**Passing Data Between Workflow Steps:**

When using `engine` mode, data automatically flows between steps:

```yaml
steps:
  # Step 1: Return data directly (NEW - simplified!)
  - tool_name: run_command
    arguments:
      engine: "javascript"
      run: |
        // Get file info (example)
        const filePath = 'C:\\data\\report.pdf';
        const fileSize = 1024;

        console.log(`Found file: ${filePath}`);

        // Just return fields directly - they auto-merge into env
        return {
          status: 'success',
          file_path: filePath,      // Becomes env.file_path
          file_size: fileSize        // Becomes env.file_size
        };

  # Step 2: Access data automatically
  - tool_name: run_command
    arguments:
      engine: "javascript"
      run: |
        // env is automatically available - no setup needed!
        console.log(`Processing: ${env.file_path} (${env.file_size} bytes)`);

        // Workflow variables also auto-available
        console.log(`Config: ${variables.max_retries}`);

        // NEW: Direct variable access also works!
        console.log(`Processing: ${file_path} (${file_size} bytes)`);
        console.log(`Config: ${max_retries}`);

        // Continue with desktop automation
        const elements = await desktop.locator('role:button').all();

        // Return more data (auto-merges to env)
        return {
          status: 'success',
          file_processed: env.file_path,
          buttons_found: elements.length
        };
```

**Important Notes on Data Passing:**

- **NEW:** `env` and `variables` are automatically injected into all scripts
- **NEW:** Non-reserved fields in return values auto-merge into env (no `set_env` wrapper needed)
- **NEW:** Valid env fields are also available as individual variables (e.g., `file_path` instead of `env.file_path`)
- Reserved fields that don't auto-merge: `status`, `error`, `logs`, `duration_ms`, `set_env`
- Data passing only works with `engine` mode (JavaScript/Python), NOT with shell commands
- Backward compatible: explicit `set_env` still works if needed
- Individual variable names must be valid JavaScript identifiers (no spaces, special chars, or reserved keywords)
- Watch for backslash escaping issues in Windows paths (may need double escaping)
- Consider combining related operations in a single step if data passing becomes complex

For complete CLI documentation, see [Terminator CLI README](../terminator-cli/README.md).

### Core Workflows: From Interaction to Structured Data

The Terminator MCP agent offers two primary workflows for automating desktop tasks. Both paths lead to the same goal: creating a >95% accuracy, 10000x faster than humans, automation.

#### 1. Iterative Development with `execute_sequence`

This is the most powerful and flexible method. You build a workflow step-by-step, using MCP tools to inspect the UI and refine your actions.

1.  **Inspect the UI**: Start by using `get_focused_window_tree` to understand the structure of your target application. This gives you the roles, names, and IDs of all elements. For performance optimization:
    - Use `tree_max_depth: 2` to limit tree depth when you only need shallow inspection
    - Use `tree_from_selector: "role:Dialog"` to get subtree from a specific element
    - Use `tree_from_selector: "true"` to start from the currently focused element
    - Use `tree_output_format: "compact_yaml"` (default) for readable format or `"verbose_json"` for full data
2.  **Build a Sequence**: Create an `execute_sequence` tool call with a series of actions (`click_element`, `type_into_element`, etc.). Use robust selectors (like `role|name` or stable `properties:AutomationId:value` selectors) whenever possible.
3.  **Capture the Final State**: Ensure the last step in your sequence is an action that returns a UI tree. The `wait_for_element` tool with `include_tree: true` is perfect for this, as it captures the application's state after your automation has run.
4.  **Extract Structured Data with `output_parser`**: Add the `output_parser` argument to your `execute_sequence` call. Write JavaScript code to parse the final UI tree and extract structured data. If successful, the tool result will contain a `parsed_output` field with your clean JSON data.

Here is an example of an `output_parser` that extracts insurance quote data from a web page:

```yaml
output_parser:
  ui_tree_source_step_id: capture_quotes_tree
  javascript_code: |
    // Find all quote groups with Image and Text children
    const results = [];

    function findElementsRecursively(element) {
        if (element.attributes && element.attributes.role === 'Group') {
            const children = element.children || [];
            const hasImage = children.some(child => 
                child.attributes && child.attributes.role === 'Image'
            );
            const hasText = children.some(child => 
                child.attributes && child.attributes.role === 'Text'
            );
            
            if (hasImage && hasText) {
                const textElements = children.filter(child => 
                    child.attributes && child.attributes.role === 'Text' && child.attributes.name
                );
                
                let carrierProduct = '';
                let monthlyPrice = '';
                
                for (const textEl of textElements) {
                    const text = textEl.attributes.name;
                    if (text.includes(':')) {
                        carrierProduct = text;
                    }
                    if (text.startsWith('$')) {
                        monthlyPrice = text;
                    }
                }
                
                if (carrierProduct && monthlyPrice) {
                    results.push({
                        carrierProduct: carrierProduct,
                        monthlyPrice: monthlyPrice
                    });
                }
            }
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

#### 2. Recording Human Actions with `record_workflow`

For simpler tasks, you can record your own actions to generate a baseline workflow.

1.  **Start Recording**: Call `record_workflow` with `action: "start"`.
2.  **Perform the Task**: Manually perform the clicks, typing, and other interactions in the target application.
3.  **Stop and Save**: Call `record_workflow` with `action: "stop"`. This returns a complete workflow JSON file containing all your recorded actions.
4.  **Refine and Parse**: The recorded workflow is a great starting point. You can then refine the selectors for robustness, add a final step to capture the UI tree, and attach an `output_parser` to extract structured data, just as you would in the iterative workflow.

### Browser DOM Inspection

The `execute_browser_script` tool enables direct JavaScript execution in browser contexts, providing access to the full HTML DOM. This is particularly useful when you need information not available in the accessibility tree.

#### When to Use DOM vs Accessibility Tree

**Use Accessibility Tree (default) when:**

- Navigating and interacting with UI elements
- Working with semantic page structure
- Building reliable automation workflows
- Performance is critical (faster, cleaner data)

**Use DOM Inspection when:**

- Extracting data attributes, meta tags, or hidden inputs
- Debugging why elements aren't appearing in accessibility tree
- Scraping structured data from specific HTML patterns
- Validating complete page structure or SEO elements

#### Basic DOM Retrieval Patterns

```javascript
// Get full HTML DOM (be mindful of size limits)
execute_browser_script({
  selector: "role:Window|name:Google Chrome",
  script: "document.documentElement.outerHTML",
});

// Get structured page information
execute_browser_script({
  selector: "role:Window|name:Google Chrome",
  script: `({
    url: window.location.href,
    title: document.title,
    html: document.documentElement.outerHTML,
    bodyText: document.body.innerText.substring(0, 1000)
  })`,
});

// Extract specific data (forms, hidden inputs, meta tags)
execute_browser_script({
  selector: "role:Window|name:Google Chrome",
  script: `({
    forms: Array.from(document.forms).map(f => ({
      id: f.id,
      action: f.action,
      method: f.method,
      inputs: Array.from(f.elements).map(e => ({
        name: e.name,
        type: e.type,
        value: e.type === 'password' ? '[REDACTED]' : e.value
      }))
    })),
    hiddenInputs: Array.from(document.querySelectorAll('input[type="hidden"]')).map(e => ({
      name: e.name,
      value: e.value
    })),
    metaTags: Array.from(document.querySelectorAll('meta')).map(m => ({
      name: m.name || m.property,
      content: m.content
    }))
  })`,
});
```

#### Handling Large DOMs

The MCP protocol has response size limits (~30KB). For large DOMs, use truncation strategies:

```javascript
execute_browser_script({
  selector: "role:Window|name:Google Chrome",
  script: `
    const html = document.documentElement.outerHTML;
    const maxLength = 30000;
    
    ({
      url: window.location.href,
      title: document.title,
      html: html.length > maxLength 
        ? html.substring(0, maxLength) + '... [truncated at ' + maxLength + ' chars]'
        : html,
      totalLength: html.length,
      truncated: html.length > maxLength
    })
  `,
});
```

#### Advanced DOM Analysis

```javascript
// Analyze page structure and extract semantic content
execute_browser_script({
  selector: "role:Window|name:Google Chrome",
  script: `
    // Remove scripts and styles for cleaner analysis
    const clonedDoc = document.documentElement.cloneNode(true);
    clonedDoc.querySelectorAll('script, style, noscript').forEach(el => el.remove());
    
    ({
      // Page metrics
      domElementCount: document.querySelectorAll('*').length,
      formCount: document.forms.length,
      linkCount: document.links.length,
      imageCount: document.images.length,
      
      // Semantic structure
      headings: Array.from(document.querySelectorAll('h1,h2,h3')).map(h => ({
        level: h.tagName,
        text: h.innerText.substring(0, 100)
      })),
      
      // Clean HTML without scripts/styles
      cleanHtml: clonedDoc.outerHTML.substring(0, 20000),
      
      // Data extraction
      jsonLd: Array.from(document.querySelectorAll('script[type="application/ld+json"]'))
        .map(s => { try { return JSON.parse(s.textContent); } catch { return null; } })
        .filter(Boolean)
    })
  `,
});
```

#### Passing Data with Environment Variables

The `execute_browser_script` tool now supports passing data through `env` and `outputs` parameters:

```javascript
// Step 1: Set environment variables in JavaScript
run_command({
  engine: "javascript",
  run: `
    return {
      set_env: {
        userName: 'John Doe',
        userId: '12345',
        apiKey: 'secret-key'
      }
    };
  `,
});

// Step 2: Use environment variables in browser script
execute_browser_script({
  selector: "role:Window",
  env: {
    userName: "{{env.userName}}",
    userId: "{{env.userId}}",
  },
  script: `
    // Parse env if it's a JSON string (for backward compatibility)
    const parsedEnv = typeof env === 'string' ? JSON.parse(env) : env;

    // Use the data - traditional way
    console.log('Processing user:', parsedEnv.userName);

    // NEW: Direct variable access also works!
    console.log('Processing user:', userName);  // Direct access
    console.log('User ID:', userId);            // No env prefix needed

    // Fill form with data
    document.querySelector('#username').value = userName;
    document.querySelector('#userid').value = userId;
    
    // Return result and set new variables
    JSON.stringify({
      status: 'form_filled',
      set_env: {
        form_submitted: 'true',
        timestamp: new Date().toISOString()
      }
    });
  `,
});
```

#### Loading Scripts from Files

You can load JavaScript from external files using the `script_file` parameter:

```javascript
// browser_scripts/extract_data.js
const parsedEnv = typeof env === "string" ? JSON.parse(env) : env;
const parsedOutputs =
  typeof outputs === "string" ? JSON.parse(outputs) : outputs;

console.log("Script loaded from file");
console.log("User:", parsedEnv?.userName);
console.log("Previous result:", parsedOutputs?.previousStep);

// Extract and return data
JSON.stringify({
  extractedData: {
    url: window.location.href,
    title: document.title,
    forms: document.forms.length,
  },
  set_env: {
    extraction_complete: "true",
  },
});

// In your workflow:
execute_browser_script({
  selector: "role:Window",
  script_file: "browser_scripts/extract_data.js",
  env: {
    userName: "{{env.userName}}",
    previousStep: "{{env.previousStep}}",
  },
});
```

#### Important Notes

1. **Chrome Extension Required**: The `execute_browser_script` tool requires the Terminator browser extension to be installed. See the installation workflow examples for automated setup.

2. **Security Considerations**: Be cautious when extracting sensitive data. The examples above redact password fields and you should follow similar practices.

3. **Performance**: DOM operations are synchronous and can be slow on large pages. Consider using specific selectors rather than traversing the entire DOM.

4. **Error Handling**: Always wrap complex DOM operations in try-catch blocks and return meaningful error messages.

5. **Data Injection**: When using `env` or `outputs` parameters, they are injected as JavaScript variables at the beginning of your script. Always parse them if they might be JSON strings.

## Local Development

To build and test the agent from the source code:

```sh
# 1. Clone the entire Terminator repository
git clone https://github.com/mediar-ai/terminator

# 2. Navigate to the agent's directory
cd terminator/terminator-mcp-agent

# 3. Install Node.js dependencies
npm install

# 4. Build the Rust binary and Node.js wrapper
npm run build

# 5. To use your local build in your MCP client, link it globally
npm install --global .
```

Now, when your MCP client runs `terminator-mcp-agent`, it will use your local build instead of the published `npm` version.

---

## Troubleshooting

- Make sure you have Node.js installed (v16+ recommended).
- For VS Code/Insiders, ensure the CLI (`code` or `code-insiders`) is available in your PATH.
- If you encounter issues, try running with elevated permissions.

### Version Compatibility Issues

**Problem**: "missing field `items`" or schema mismatch errors

**Solution**: Ensure you're using the latest MCP server version:

```bash
# Force latest version in CLI
terminator mcp run workflow.yml --command "npx -y terminator-mcp-agent@latest"

# Update MCP client configuration to use @latest
{
  "mcpServers": {
    "terminator-mcp-agent": {
      "command": "npx",
      "args": ["-y", "terminator-mcp-agent@latest"]
    }
  }
}

# Clear npm cache if needed
npm cache clean --force
```

### CLI Integration Issues

**Problem**: CLI commands not working or connection errors

**Solution**: Test MCP connectivity step by step:

```bash
# Test basic connectivity
terminator mcp exec get_applications

# Test with verbose logging
terminator mcp run workflow.yml --verbose

# Test with dry run first
terminator mcp run workflow.yml --dry-run

# Use HTTP connection for debugging
terminator mcp run workflow.yml --url http://localhost:3000/mcp
```

### JavaScript Execution Issues

**Problem**: JavaScript code fails or can't access desktop APIs

**Solution**: Verify JavaScript execution and API access:

```bash
# Test basic JavaScript execution via run_command engine mode
terminator mcp exec run_command '{"engine": "javascript", "run": "return {test: true};"}'

# Test desktop API access with node engine
terminator mcp exec run_command '{"engine": "node", "run": "const elements = await desktop.locator(\\\"role:button\\\").all(); return {count: elements.length};"}'

# Test Python engine
terminator mcp exec run_command '{"engine": "python", "run": "return {\\\"py\\\": True}"}'

# Debug with verbose logging
terminator mcp run workflow.yml --verbose
```

### Workflow File Issues

**Problem**: Workflow parsing errors or unexpected behavior

**Solution**: Validate workflow structure:

```bash
# Validate workflow syntax
terminator mcp run workflow.yml --dry-run

# Test with minimal workflow first
echo 'steps: [{tool_name: get_applications}]' > test.yml
terminator mcp run test.yml

# Check both YAML and JSON formats work
terminator mcp run workflow.yml   # YAML
terminator mcp run workflow.json  # JSON
```

### Platform-Specific Issues

**Windows**:

- Ensure Windows UI Automation APIs are available
- Run with administrator privileges if accessibility features are restricted
- Check Windows Defender/antivirus isn't blocking automation

**macOS**:

- Grant accessibility permissions in System Preferences > Security & Privacy
- Ensure the terminal/IDE has accessibility access
- Check macOS version compatibility (10.14+ recommended)

**Linux**:

- Ensure AT-SPI (assistive technology) is enabled
- Install required packages: `sudo apt-get install at-spi2-core`
- Check desktop environment compatibility (GNOME, KDE, XFCE supported)

### Virtual Display Support (Headless VMs)

Terminator MCP Agent includes virtual display support for running on headless VMs without requiring RDP connections. This enables scalable automation on cloud platforms like Azure, AWS, and GCP.

**How It Works**:

The agent automatically detects headless environments and initializes a virtual display context that Windows UI Automation APIs can interact with. This allows full UI automation capabilities even when no physical display or RDP session is active.

**Activation**:

Virtual display activates automatically when:

- Environment variable `TERMINATOR_HEADLESS=true` is set
- No console window is available (common in VM/container scenarios)
- Running as a Windows service or scheduled task

**Configuration**:

```bash
# Enable virtual display mode
export TERMINATOR_HEADLESS=true

# Run the MCP agent
npx -y terminator-mcp-agent
```

**Use Cases**:

- Running multiple automation agents on VMs without RDP overhead
- CI/CD pipelines in cloud environments
- Scalable automation farms on Azure/AWS/GCP
- Containerized automation workloads

**Requirements**:

- Windows Server 2016+ or Windows 10/11
- .NET Framework 4.7.2+
- UI Automation APIs available (included in Windows)

The virtual display manager creates a memory-based display context that satisfies Windows UI Automation requirements, enabling terminator to enumerate and interact with UI elements as if a physical display were present.

### Performance Optimization

**Large UI Trees**:

- Use specific selectors instead of broad element searches
- Implement delays between rapid operations
- Consider using `include_tree: false` for intermediate steps
- For tree extraction tools, optimize with:
  - `tree_max_depth: 2` - Limit depth for large trees
  - `tree_from_selector: "role:List"` - Get subtree from specific element
  - `tree_from_selector: "true"` - Start from focused element
  - `tree_output_format: "compact_yaml"` - Readable format (default) or `"verbose_json"` for full data

**JavaScript Performance**:

- Use `quickjs` engine for lightweight operations
- Use `nodejs` engine only when full APIs are needed
- Implement `sleep()` delays in loops to prevent overwhelming the UI

For additional help, see the [Terminator CLI documentation](../terminator-cli/README.md) or open an issue on GitHub.

---

## 📚 Full `execute_sequence` Reference & Sample Workflow

> **Why another example?** The quick start above shows the concept, but many users asked for a fully-annotated workflow schema. The example below automates the Windows **Calculator** app—so it is 100% safe to share and does **not** reveal any private customer data. Feel free to copy-paste and adapt it to your own application.

### 1. Anatomy of an `execute_sequence` Call

```jsonc
{
  "tool_name": "execute_sequence",
  "arguments": {
    "variables": {
      // 1️⃣ Re-usable inputs with type metadata
      "app_path": {
        "type": "string",
        "label": "Calculator EXE Path",
        "default": "calc.exe"
      },
      "first_number": {
        "type": "string",
        "label": "First Number",
        "default": "42"
      },
      "second_number": {
        "type": "string",
        "label": "Second Number",
        "default": "8"
      }
    },
    "inputs": {
      // 2️⃣ Concrete values for *this run*
      "app_path": "calc.exe",
      "first_number": "42",
      "second_number": "8"
    },
    "selectors": {
      // 3️⃣ Human-readable element shortcuts
      "calc_window": "role:Window|name:Calculator",
      "btn_clear": "role:Button|name:Clear",
      "btn_plus": "role:Button|name:Plus",
      "btn_equals": "role:Button|name:Equals"
    },
    "steps": [
      // 4️⃣ Ordered actions & control flow
      {
        "tool_name": "open_application",
        "arguments": { "path": "${{app_path}}" }
      },
      {
        "tool_name": "click_element", // 4a. Make sure the UI is reset
        "arguments": { "selector": "${{selectors.btn_clear}}" },
        "continue_on_error": true
      },
      {
        "group_name": "Enter First Number", // 4b. Groups improve logs
        "steps": [
          {
            "tool_name": "type_into_element",
            "arguments": {
              "selector": "${{selectors.calc_window}}",
              "text_to_type": "${{first_number}}"
            }
          }
        ]
      },
      {
        "tool_name": "click_element",
        "arguments": { "selector": "${{selectors.btn_plus}}" }
      },
      {
        "group_name": "Enter Second Number",
        "steps": [
          {
            "tool_name": "type_into_element",
            "arguments": {
              "selector": "${{selectors.calc_window}}",
              "text_to_type": "${{second_number}}"
            }
          }
        ]
      },
      {
        "tool_name": "click_element",
        "arguments": { "selector": "${{selectors.btn_equals}}" }
      },
      {
        "tool_name": "wait_for_element", // 4c. Capture final UI tree
        "arguments": {
          "selector": "${{selectors.calc_window}}",
          "condition": "exists",
          "include_tree": true,
          "timeout_ms": 2000
        }
      }
    ],
    "output_parser": {
      // 5️⃣ Turn the tree into clean JSON
      "javascript_code": "// Extract calculator display value\nconst results = [];\n\nfunction findElementsRecursively(element) {\n    if (element.attributes && element.attributes.role === 'Text') {\n        const item = {\n            displayValue: element.attributes.name || ''\n        };\n        results.push(item);\n    }\n    \n    if (element.children) {\n        for (const child of element.children) {\n            findElementsRecursively(child);\n        }\n    }\n}\n\nfindElementsRecursively(tree);\nreturn results;"
    }
  }
}
```

### 2. Key Concepts at a Glance

1. **Variables vs. Inputs** – Declare once, override per-run. This is perfect for parameterizing CI pipelines or A/B test data.
2. **Selectors** – Give every important UI element a _nickname_. It makes long workflows readable and easy to maintain.
3. **Templating** – `${{ ... }}` (GitHub Actions-style) _or_ legacy `{{ ... }}` lets you reference **any** key inside `variables`, `inputs`, or `selectors`. Both syntaxes are supported; the engine uses Mustache-style rendering.
4. **Groups & Control Flow** – Add `group_name`, `skippable`, `if`, or `continue_on_error` to any step for advanced branching.
5. **Output Parsing** – Always end with a step that includes the UI tree, then use the declarative JSON DSL to mine the data you need.

### 3. State Persistence & Partial Execution

The `execute_sequence` tool supports powerful features for workflow debugging and resumption:

#### Partial Execution with Step Ranges

You can run specific portions of a workflow using `start_from_step` and `end_at_step` parameters:

```jsonc
{
  "tool_name": "execute_sequence",
  "arguments": {
    "url": "file://path/to/workflow.yml",
    "start_from_step": "read_json_file",    // Start from this step ID
    "end_at_step": "fill_journal_entries",  // Stop after this step (inclusive)
    "follow_fallback": false,               // Don't follow fallback_id beyond end_at_step (default: false)
    "execute_jumps_at_end": false          // Don't execute jumps at end_at_step boundary (default: false)
  }
}
```

**Examples:**
- Run single step: Set both `start_from_step` and `end_at_step` to the same ID
- Run step range: Set different IDs for start and end
- Run from step to end: Only set `start_from_step`
- Run from beginning to step: Only set `end_at_step`
- Debug without fallback: Use `follow_fallback: false` to prevent jumping to troubleshooting steps when a bounded step fails
- Allow jumps at boundary: Use `execute_jumps_at_end: true` to execute jump conditions even at the `end_at_step` boundary (by default, jumps are skipped at the boundary for predictable execution)

#### Automatic State Persistence

When using `file://` URLs, the workflow state (environment variables) is automatically saved to a `.workflow_state` folder:

1. **State is saved** after each step that modifies environment variables via `set_env` or has a tool result with an ID
2. **State is loaded** when starting from a specific step
3. **Location**: `.workflow_state/<workflow_hash>.json` in the workflow's directory
4. **Tool results** from all tools (not just scripts) are automatically stored as `{step_id}_result` and `{step_id}_status`

This enables:
- **Debugging**: Run steps individually to inspect state between executions
- **Recovery**: Resume failed workflows from the last successful step
- **Testing**: Test specific steps without re-running the entire workflow

#### Data Passing Between Steps

Steps can pass data using multiple methods:

##### 1. Tool Result Storage (NEW)

ALL tools with an `id` field automatically store their results in the environment:

```yaml
steps:
  # Any tool with an ID stores its result
  - id: check_apps
    tool_name: get_applications
    arguments:
      include_tree: false

  # Access the result in JavaScript
  - tool_name: run_command
    arguments:
      engine: javascript

# Tree Parameter Examples - Performance Optimization
- tool_name: get_window_tree
  arguments:
    pid: 1234
    tree_max_depth: 2  # Only get 2 levels deep

- tool_name: get_focused_window_tree
  arguments:
    tree_from_selector: "role:Dialog"  # Start tree from first dialog
    tree_max_depth: 3  # Limit depth from that point

- tool_name: get_focused_window_tree
  arguments:
    tree_from_selector: "true"  # Start from focused element

# Backward compatible - still works
- tool_name: get_window_tree
  arguments:
    pid: 1234
    include_tree: true  # Simple boolean form
      run: |
        // Direct variable access - auto-injected!
        const apps = check_apps_result || [];
        const status = check_apps_status; // "success" or "error"
        console.log(`Found ${apps[0]?.applications?.length} apps`);
```

##### 2. Script Return Values

Steps can pass data using the `set_env` mechanism in `run_command` with engine mode:

```javascript
// Step 12: Read and process data
return {
  set_env: {
    file_path: "C:/data/input.json",
    journal_entries: JSON.stringify(entries),
    total_debit: "100.50"
  }
};

// Step 13: Use the data (NEW - simplified access!)
const filePath = file_path;  // Direct access, no {{env.}} needed!
const entries = JSON.parse(journal_entries);
const debit = total_debit;
```

### 4. Running the Workflow

1. Ensure the Terminator MCP agent is running (it will auto-start in supported editors).
2. Send the JSON above as the body of an `execute_sequence` tool call from your LLM or test harness.
3. Inspect the response: if parsing succeeds you'll see something like

### Realtime events (SSE)

When running with the HTTP transport, you can subscribe to realtime workflow events at a separate endpoint outside `/mcp`:

- SSE endpoint: `/events`
- Emits JSON payloads for: `sequence` (start/end), `sequence_progress`, and `sequence_step` (begin/end)

Example in Node.js:

```js
import EventSource from "eventsource";
const es = new EventSource("http://127.0.0.1:3000/events");
es.onmessage = (e) => console.log("event", e.data);
```

```jsonc
{
  "parsed_output": {
    "displayValue": "50" // 42 + 8
  }
}
```

### 5. Working with Tool Results

Every tool that has an `id` field automatically stores its result for use in later steps:

```yaml
steps:
  # Capture browser DOM
  - id: capture_dom
    tool_name: execute_browser_script
    arguments:
      selector: "role:Window"
      script: "return document.documentElement.innerHTML;"

  # Validate an element exists
  - id: check_button
    tool_name: validate_element
    arguments:
      selector: "role:Button|name:Submit"

  # Use both results in script
  - tool_name: run_command
    arguments:
      engine: javascript
      run: |
        // All tool results are auto-injected as variables
        const dom = capture_dom_result?.content || '';
        const buttonExists = check_button_status === 'success';

        if (buttonExists) {
          const button = check_button_result[0]?.element;
          console.log(`Submit button at: ${button?.bounds?.x}, ${button?.bounds?.y}`);
        }

        return { dom_length: dom.length, has_button: buttonExists };
```

Tool results are accessible as:
- `{step_id}_result`: The tool's return value (content, element info, etc.)
- `{step_id}_status`: Either "success" or "error"

### 6. Tips for Production Workflows

- **Never hard-code credentials** – use environment variables or your secret manager.
- **Keep workflows short** – <100 steps is ideal. Break large tasks into multiple sequences.
- **Capture errors** – `continue_on_error` is useful, but also check `{step_id}_status` for tool failures.
- **Version control** – Store workflow JSON in a repo and use PR reviews just like regular code.
- **Use step IDs** – Give meaningful IDs to steps whose results you'll need later.

## 🔍 Troubleshooting & Debugging

### Finding MCP Server Logs

MCP logs are saved to:
- **Windows:** `%LOCALAPPDATA%\claude-cli-nodejs\Cache\<encoded-project-path>\mcp-logs-terminator-mcp-agent\`
- **macOS/Linux:** `~/.local/share/claude-cli-nodejs/Cache/<encoded-project-path>/mcp-logs-terminator-mcp-agent/`

Where `<encoded-project-path>` is your project path with special chars replaced (e.g., `C--Users-username-project`).
Note: Logs are saved as `.txt` files, not `.log` files.

**Read logs:**
```powershell
# Windows - Find and read latest logs (run in PowerShell)
Get-ChildItem (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'claude-cli-nodejs\Cache\*\mcp-logs-terminator-mcp-agent\*.txt') | Sort-Object LastWriteTime -Descending | Select-Object -First 1 | Get-Content -Tail 50
```

### Enable Debug Logging

In your Claude MCP configuration (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "terminator-mcp-agent": {
      "command": "path/to/terminator-mcp-agent",
      "env": {
        "LOG_LEVEL": "debug",  // or "info", "warn", "error"
        "RUST_BACKTRACE": "1"   // for stack traces on errors
      }
    }
  }
}
```

### Common Debug Scenarios

| Issue | What to Look For in Logs |
|-------|--------------------------|
| Workflow failures | Search for `fallback_id` triggers and `critical_error_occurred` |
| Element not found | Look for selector resolution attempts, `find_element` timeouts |
| Browser script errors | Check for `EVAL_ERROR`, Promise rejections, JavaScript exceptions |
| Binary version issues | Startup logs show binary path and build timestamp |
| MCP connection lost | Check for panic messages, ensure binary path is correct |

### Fallback Mechanism

Workflows support `fallback_id` to handle errors gracefully:
- If a step fails and has `fallback_id`, it jumps to that step instead of stopping
- Without `fallback_id`, errors may set `critical_error_occurred` and skip remaining steps
- Use `troubleshooting:` section for recovery steps only accessed via fallback

> Need more help? Browse the examples under `examples/` in this repo or open a discussion on GitHub.

## Documentation

### Workflow Development

- **[Workflow Output Structure](docs/WORKFLOW_OUTPUT_STRUCTURE.md)**: Detailed documentation on the expected output structure for workflows, including:
  - How to structure `parsed_output` for proper CLI rendering
  - Success/failure indicators and business logic validation
  - Data extraction patterns and error handling
  - Integration with CLI and backend systems

### Additional Resources

- **[CLI Documentation](../terminator-cli/README.md)**: Command-line interface for executing workflows
- **[Examples](examples/)**: Sample workflows and use cases
- **[API Reference](https://github.com/mediar-ai/terminator#api)**: Core Terminator library documentation
