# terminator 🤖

https://github.com/user-attachments/assets/00329105-8875-48cb-8970-a62a85a9ebd0

<p align="center">
  <a href="https://discord.gg/dU9EBuw7Uq">
    <img src="https://img.shields.io/discord/823813159592001537?color=5865F2&logo=discord&logoColor=white&style=flat-square" alt="Join us on Discord">
  </a>
  <a href="https://docs.screenpi.pe/terminator/introduction">
    <img src="https://img.shields.io/badge/read_the-docs-blue" alt="Docs">
  </a>
  <a href="https://www.youtube.com/@mediar_ai">
    <img src="https://img.shields.io/badge/YouTube-@mediar__ai-FF0000?logo=youtube&logoColor=white&style=flat-square" alt="YouTube @mediar_ai">
  </a>
  <a href="https://crates.io/crates/terminator-rs">
    <img src="https://img.shields.io/crates/v/terminator-rs.svg" alt="Crates.io - terminator-rs">
  </a>
  <a href="https://crates.io/crates/terminator-workflow-recorder">
    <img src="https://img.shields.io/crates/v/terminator-workflow-recorder.svg" alt="Crates.io - workflow recorder">
  </a>
</p>

<p align="center">
  <a href="https://insiders.vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D">
    <img alt="Install in VS Code" src="https://img.shields.io/badge/VS_Code-VS_Code?style=flat-square&label=Install%20MCP&color=0098FF">
  </a>
  <a href="https://insiders.vscode.dev/redirect?url=vscode-insiders%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D">
    <img alt="Install in VS Code Insiders" src="https://img.shields.io/badge/VS_Code_Insiders-VS_Code_Insiders?style=flat-square&label=Install%20MCP&color=24bfa5">
  </a>
  <a href="https://cursor.com/install-mcp?name=terminator-mcp-agent&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsInRlcm1pbmF0b3ItbWNwLWFnZW50Il19">
    <img alt="Install in Cursor" src="https://img.shields.io/badge/Cursor-Cursor?style=flat-square&label=Install%20MCP&color=22272e">
  </a>
</p>


>Computer use SDK for building agents that learn from human screen recordings. Accessibility-first. Cross-platform (Windows/macOS/Linux), near-deterministic.

## ⚡ TL;DR — Hello World Example

> Skip the boilerplate. This is the fastest way to feel the magic.

### 🤖 Natural Language Control with MCP Client (Recommended)

Control your computer using natural language through Claude or other LLMs:

```
# Just talk to it:
💬 You: Open Notepad and write a haiku about robots
🤖 Claude: I'll help you open Notepad and write a haiku...
   [Opening Notepad...]
   [Typing haiku...]
   ✅ Done! I've written a haiku about robots in Notepad.

💬 You: Open Twitter and comment on people posts with an e/acc style 
🤖 Claude: Sure I'll do that
   [Opening x.com in Chrome...]
   ✅ I commented on Karpathy post about aliens
```

**Installation:**

```bash
# Option 1: Download pre-built MCP agent (Windows)
# Download from: https://github.com/mediar-ai/terminator/releases/latest
# Extract terminator-mcp-windows-x86_64.zip and add to PATH

# Option 2: Build from source (requires Rust)
cargo build --release --bin terminator-mcp-agent

# Install Python dependencies
pip install "mcp[client]" anthropic python-dotenv

# Set your API key
export ANTHROPIC_API_KEY='your-key-here'

# Run the client (automatically starts MCP agent)
python examples/python_mcp_client.py

# or the HTTP transport 
terminator-mcp-agent -t http
python examples/python_mcp_client_http.py
```

[See the full MCP client example →](examples/python_mcp_client.py)

### 🐍 Direct Python SDK

For programmatic control without AI:

```python
import terminator

# Control applications programmatically
desktop = terminator.Desktop()
desktop.open_application('calc')
desktop.locator('name:Seven').click()
desktop.locator('name:Plus').click()  
desktop.locator('name:Three').click()
desktop.locator('name:Equals').click()
# Result: 10 appears in calculator
```

**Installation:**
```bash
pip install terminator.py
```

### 🟦 TypeScript / Node.js SDK

```typescript
const { Desktop } = require('terminator.js');

// Async/await for modern control flow
const desktop = new Desktop();
await desktop.openApplication('notepad');
await desktop.locator('name:Edit').typeText('Hello from TypeScript!');
await desktop.pressKey('{Ctrl}s'); // Save
```

**Installation:**
```bash
npm install terminator.js
# or: bun add terminator.js
```

### 🧠 What is Terminator?

Terminator is an AI-first Playwright-style SDK for automating operating systems.

- 🪟 Built for Windows, with partial support on Linux and macOS
- 🤖 Learns deterministically from screen recordings of real workflows
- 🧠 Designed for AI agents—not humans
- ⚡ Uses OS-level accessibility APIs, with OCR/Vision as fallback
- 🧩 Supports TypeScript, Python, MCP, and Rust
- 📈 Scans the UI in ~80ms—up to 10,000x faster and cheaper than a human

Terminator runs "headless" by default. It doesn't require a visible screen, relying instead on accessibility layers (like UI Automation on Windows) to interact with apps.

## 🎯 Building with MCP (Model Context Protocol)

The **MCP integration** is the recommended way to build AI-powered applications with Terminator. It provides a standardized interface between LLMs (like Claude) and desktop automation tools.

### System Requirements

- **Windows**: Pre-built binaries available (recommended)
- **macOS/Linux**: Build from source with Rust
- **Python 3.8+** for the client
- **API Key** from Anthropic, OpenAI, or other LLM provider

### Why MCP?

- **Natural Language Interface**: Control your desktop using plain English
- **AI-Powered Decision Making**: The LLM decides which tools to use and when
- **Complex Workflows**: Handle multi-step operations with a single command
- **Error Recovery**: AI can adapt when things don't go as expected
- **Tool Chaining**: Automatically sequences multiple operations
- **(Soon) Near determinism and production ready**: We build the tools to make the MCP client near deterministic and production ready through generated workflows of human screen recordings

### Quick Start with MCP

1. **Setup** (one-time):
   ```bash
   # Clone the repo
   git clone https://github.com/mediar-ai/terminator
   cd terminator
   
   # Build the MCP agent (or download from releases)
   cargo build --release --bin terminator-mcp-agent
   
   # Install Python dependencies
   pip install "mcp[client]" anthropic python-dotenv
   ```

2. **Run**:
   ```bash
   # Set your API key
   export ANTHROPIC_API_KEY='your-key-here'
   
   # Start the client (auto-starts MCP agent)
   python examples/python_mcp_client.py
   ```

3. **Try these examples**:
   - "Open Chrome and search for 'best pizza near me'"
   - "Create a new text file on the desktop and write meeting notes"
   - "Take a screenshot and tell me what applications are running"
   - "Open Calculator and compute 15% tip on $47.50"

### How It Works

The MCP client connects Claude (or other LLMs) to Terminator's automation capabilities:

```
You → "Open Notepad and write a poem" → Claude → MCP Tools → Desktop Automation
                                            ↓
                                    Decides to use:
                                    1. open_application
                                    2. wait_for_element
                                    3. type_into_element
```

### Available MCP Tools

The MCP agent exposes 40+ tools for desktop automation, including:
- **Application Control**: `open_application`, `close_element`, `get_applications`
- **UI Interaction**: `click_element`, `type_into_element`, `scroll_element`
- **Navigation**: `wait_for_element`, `validate_element`, `get_window_tree`
- **Data Capture**: `capture_screen`, `capture_element_screenshot`, `get_clipboard`
- **Advanced**: `mouse_drag`, `press_key`, `select_option`, `set_range_value`

[Full MCP tool documentation →](terminator-mcp-agent/README.md)



### Integrating with AI Applications

The MCP client example shows how to build conversational AI applications. You can:

1. **Build Custom Agents**: Create specialized automation agents for specific workflows
2. **Integrate with Existing Apps**: Add desktop automation to your AI applications
3. **Use Any LLM**: While the example uses Claude, you can adapt it for GPT, Gemini, or open-source models
4. **Deploy at Scale**: Run multiple MCP agents for different users or tasks

See the [Python MCP client source](examples/python_mcp_client.py) for a complete implementation you can customize.

### 🌐 Remote Desktop Control via HTTP + ngrok

Control a computer remotely by exposing the MCP server over HTTP and tunneling through ngrok:

**On the Host Computer (being controlled):**

```bash
# 1. Start MCP server in HTTP mode
terminator-mcp-agent --transport http --port 3000

# 2. Expose via ngrok (in another terminal)
ngrok http 3000
# Copy the HTTPS URL (e.g., https://abc123.ngrok-free.app)
```

**On the Client Computer (controller):**

```python
# Install dependencies
pip install "mcp[client]" httpx anthropic

# Connect to remote MCP server
python examples/remote_mcp_client.py https://abc123.ngrok-free.app

# Now control the remote desktop:
💬 You: Open Notepad and type "Hello from remote!"
🤖 Claude: Opening Notepad on the remote computer...
```

**Key Points:**
- The host runs `terminator-mcp-agent` with `--transport http`
- ngrok creates a secure tunnel to expose port 3000
- The client connects using the ngrok HTTPS URL
- All 40+ MCP tools work remotely (screenshots, typing, clicking, etc.)

⚠️ **Security**: ngrok URLs are public. Only share with trusted users and shut down when not in use.

In production prefer using auth, open port, VM, or use more heavy stuff like Kubernetes.

## Direct SDK Usage (Alternative)

While MCP is recommended for AI-powered automation, you can also use the SDKs directly for programmatic control:

- **Python SDK**: For long running tasks
- **TypeScript SDK**: For web applications and Node.js services  
- **Rust SDK**: For high-performance system integration

These provide fine-grained control but require you to specify each action explicitly.

## Feature Support

While Terminator aims for full cross-platform support, current capabilities vary by OS. Windows is the primary development target and has the most complete feature set.

| Feature                  | Windows | macOS | Linux | Notes                                        |
| ------------------------ | :-----: | :---: | :---: | -------------------------------------------- |
| **Core Automation**      |         |       |       |                                              |
| Element Locators         |    ✅   |  🟡   |  🟡   | Find elements by `name`, `role`, `window`, etc. |
| UI Actions (`click`, `type`) |    ✅   |  🟡   |  🟡   | Core interactions with UI elements.          |
| Application Management   |    ✅   |  🟡   |  🟡   | Launch, list, and manage applications. |
| Window Management        |    ✅   |  🟡   |  🟡   | Get active window, list windows.             |
| **Advanced Features**    |         |       |       |                                              |
| Workflow Recording       |    ✅   |  ❌   |  ❌   | Record human workflows for deterministic automation.     |
| Monitor Management       |    ✅   |  🟡   |  🟡   | Multi-display support.                       |
| Screen & Element Capture |    ✅   |  ✅   |  🟡   | Take screenshots of displays or elements.     |
| **Language Bindings**    |         |       |       |                                              |
| Python (`terminator.py`) |    ✅   |  ✅   |  ✅   | `pip install terminator.py`                  |
| TypeScript (`terminator.js`) |    ✅   |  ✅   |  ✅   | `npm i terminator.js`                        |
| MCP (`terminator-mcp-agent`) |    ✅   |  ✅   |  ✅   | `npx -y terminator-mcp-agent --add-to-app [app]`                        |
| Rust (`terminator-rs`) |    ✅   |  ✅   |  ✅   | `cargo add terminator-rs`                        |

**Legend:**
- ✅: **Supported** - The feature is stable and well-tested.
- 🟡: **Partial / Experimental** - The feature is in development and may have limitations.
- ❌: **Not Supported** - The feature is not yet available on this platform.

## 🕵️ How to Inspect Accessibility Elements (like `name:Seven`)

To create reliable selectors (e.g. `name:Seven`, `role:Button`, `window:Calculator`), you need to inspect the Accessibility Tree of your OS. Here's how to explore UI elements on each platform:

### 🪟 Windows

* **Tool:** [Accessibility Insights for Windows](https://accessibilityinsights.io/downloads/)
* **Alt:** [Inspect.exe](https://learn.microsoft.com/en-us/windows/win32/winauto/inspect-objects) (comes with Windows SDK)
* **Usage:** Open the app you want to inspect → launch Accessibility Insights → hover or use keyboard navigation to explore the UI tree (Name, Role, ControlType, AutomationId).

> These tools show you the `Name`, `Role`, `ControlType`, and other metadata used in Terminator selectors.

---

### 🍎 macOS

* **Tool:** [Accessibility Inspector](https://developer.apple.com/documentation/xcode/accessibility_inspector)
* **Usage:** Comes with Xcode → Open `Xcode > Open Developer Tool > Accessibility Inspector` → Use the target icon to explore UI elements on screen.

---

### 🐧 Linux

* **Tool:** [Accerciser](https://wiki.gnome.org/Apps/Accerciser)
* **Install:**

  ```bash
  sudo apt install accerciser
  ```
* **Usage:** Launch Accerciser → Select the window/app → Browse the accessible widget tree.

---

### 💡 Tip

Once you identify the structure of your UI:

```python
# Sample pattern
desktop.locator('window:Calculator')
       .locator('role:Button')
       .locator('name:Seven')
```

You can build and debug selector paths incrementally using `.locator()` chaining.

## Explore Further

- https://github.com/mediar-ai/terminator-typescript-examples
- https://github.com/mediar-ai/terminator-python-examples
- https://github.com/mediar-ai/terminator/examples

## contributing

contributions are welcome! please feel free to submit issues and pull requests. many parts are experimental, and help is appreciated. join our [discord](https://discord.gg/dU9EBuw7Uq) to discuss.

## businesses 

if you want desktop automation at scale for your business, [let's talk](https://mediar.ai)
