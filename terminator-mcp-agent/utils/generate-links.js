#!/usr/bin/env node

const configObj = {
  "terminator-mcp-agent": {
    command: "npx",
    args: ["-y", "terminator-mcp-agent"],
  },
};

const configJSON = JSON.stringify(configObj);
const configBase64 = Buffer.from(configJSON).toString("base64");

const cursorWebUrl = `https://cursor.com/install-mcp?name=terminator-mcp-agent&config=${encodeURIComponent(configBase64)}`;

const urlForVSCode = `vscode:mcp/install?${encodeURIComponent(configJSON)}`;
const urlForVSCodeGithub = `https://insiders.vscode.dev/redirect?url=${encodeURIComponent(urlForVSCode)}`;

const urlForVSCodeInsiders = `vscode-insiders:mcp/install?${encodeURIComponent(configJSON)}`;
const urlForVSCodeInsidersGithub = `https://insiders.vscode.dev/redirect?url=${encodeURIComponent(urlForVSCodeInsiders)}`;

// Just log the raw URLs, in order
console.log(urlForVSCodeGithub);
console.log(urlForVSCodeInsidersGithub);
console.log(cursorWebUrl);
