# Terminator CLI

The Terminator CLI is a powerful command-line tool for managing the Terminator project, including version management, releases, **Azure VM deployment**, and **MCP server interaction**.

## Features

- 📦 **Version Management**: Bump and sync versions across all packages
- 🏷️ **Release Automation**: Tag and release with a single command
- ☁️ **Azure VM Deployment**: One-liner to deploy Windows VMs with MCP server
- 🤖 **MCP Client**: Chat with MCP servers over HTTP or stdio
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

### Control remote computer through chat (MCP client)

1. run the MCP server on your remote machine
2. open port or ngrok
3. `terminator mcp ai-chat --url https://xxx/mcp`

<img width="1512" alt="Screenshot 2025-07-04 at 1 49 10 PM" src="https://github.com/user-attachments/assets/95355099-0130-4702-bd11-0278db181253" />



