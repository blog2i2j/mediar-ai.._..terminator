## Terminator MCP Agent

<!-- BADGES:START -->

[<img alt="Install in VS Code" src="https://img.shields.io/badge/VS_Code-VS_Code?style=flat-square&label=Install%20Server&color=0098FF">](https://insiders.vscode.dev/redirect?url=vscode%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D)
[<img alt="Install in VS Code Insiders" src="https://img.shields.io/badge/VS_Code_Insiders-VS_Code_Insiders?style=flat-square&label=Install%20Server&color=24bfa5">](https://insiders.vscode.dev/redirect?url=vscode-insiders%3Amcp%2Finstall%3F%257B%2522terminator-mcp-agent%2522%253A%257B%2522command%2522%253A%2522npx%2522%252C%2522args%2522%253A%255B%2522-y%2522%252C%2522terminator-mcp-agent%2522%255D%257D%257D)
[<img alt="Install in Cursor" src="https://img.shields.io/badge/Cursor-Cursor?style=flat-square&label=Install%20Server&color=22272e">](https://cursor.com/install-mcp?name=terminator-mcp-agent&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsInRlcm1pbmF0b3ItbWNwLWFnZW50Il19)

<!-- BADGES:END -->

A Model Context Protocol (MCP) server that provides desktop GUI automation capabilities using the [Terminator](https://github.com/mediar-ai/terminator) library. This server enables LLMs and agentic clients to interact with Windows, macOS, and Linux applications through structured accessibility APIs—no vision models or screenshots required.

### Key Features

- **Fast and lightweight**. Uses OS-level accessibility APIs, not pixel-based input.
- **LLM/agent-friendly**. No vision models needed, operates purely on structured data.
- **Deterministic automation**. Avoids ambiguity common with screenshot-based approaches.
- **Multi-platform**. Supports Windows (full), macOS (partial), Linux (partial).

### Requirements

- Node.js 16 or newer
- VS Code, Cursor, Windsurf, Claude Desktop, or any other MCP client

### Getting started

First, install the Terminator MCP server with your client. A typical configuration looks like this:

```json
{
  "mcpServers": {
    "terminator-mcp-agent": {
      "command": "npx",
      "args": ["-y", "terminator-mcp-agent"]
    }
  }
}
```

You can also use the CLI to configure your app automatically:

```sh
npx -y terminator-mcp-agent --add-to-app [app]
```

Replace `[app]` with one of:

- cursor
- claude
- vscode
- insiders
- windsurf
- cline
- roocode
- witsy
- enconvo
- boltai
- amazon-bedrock
- amazonq

If you omit `[app]`, the CLI will prompt you to select from all available options.

---

<img width="1512" alt="Screenshot 2025-04-16 at 9 29 42 AM" src="https://github.com/user-attachments/assets/457ebaf2-640c-4f21-a236-fcb2b92748ab" />

MCP is useful to test out the `terminator` lib and see what you can do. You can use any model.

<br>

## Development

If you want to build and test the agent locally, clone the repo and run:

```sh
git clone https://github.com/mediar-ai/terminator
cd terminator/terminator-mcp-agent
npm install
npm run build
npm install --global .
```

You can then use the CLI as above.

---

## Troubleshooting

- Make sure you have Node.js installed (v16+ recommended).
- For VS Code/Insiders, ensure the CLI (`code` or `code-insiders`) is available in your PATH.
- If you encounter issues, try running with elevated permissions or check the config file paths above.

---
