#!/usr/bin/env node

const { spawn, execSync } = require("child_process");
const path = require("path");
const fs = require("fs");
const os = require("os");
const readline = require("readline");

function getPlatformInfo() {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "win32" && arch === "x64")
    return {
      pkg: "terminator-mcp-win32-x64-msvc",
      bin: "terminator-mcp-agent.exe",
      npmDir: "win32-x64-msvc",
    };
  if (platform === "linux" && arch === "x64")
    return {
      pkg: "terminator-mcp-linux-x64-gnu",
      bin: "terminator-mcp-agent",
      npmDir: "linux-x64-gnu",
    };
  if (platform === "darwin" && arch === "x64")
    return {
      pkg: "terminator-mcp-darwin-x64",
      bin: "terminator-mcp-agent",
      npmDir: "darwin-x64",
    };
  if (platform === "darwin" && arch === "arm64")
    return {
      pkg: "terminator-mcp-darwin-arm64",
      bin: "terminator-mcp-agent",
      npmDir: "darwin-arm64",
    };
  throw new Error(`Unsupported platform: ${platform} ${arch}`);
}

function getMcpServerEntry() {
  return {
    command: "npx",
    args: ["-y", "terminator-mcp-agent"],
  };
}

function addToCursorConfig() {
  const home = os.homedir();
  const configDir = path.join(home, ".cursor");
  const configFile = path.join(configDir, "mcp.json");
  if (!fs.existsSync(configDir)) {
    fs.mkdirSync(configDir, { recursive: true });
  }
  let config = {};
  if (fs.existsSync(configFile)) {
    try {
      config = JSON.parse(fs.readFileSync(configFile, "utf8"));
    } catch (e) {
      config = {};
    }
  }
  if (!config.mcpServers || typeof config.mcpServers !== "object") {
    config.mcpServers = {};
  }
  config.mcpServers["terminator-mcp-agent"] = getMcpServerEntry();
  fs.writeFileSync(configFile, JSON.stringify(config, null, 2));
  console.log(`Cursor configuration saved to ${configFile}`);
}

function addToClaudeConfig() {
  const platform = process.platform;
  let configDir, configFile;
  if (platform === "win32") {
    const appData =
      process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming");
    configDir = path.join(appData, "Claude");
    configFile = path.join(configDir, "claude_desktop_config.json");
  } else if (platform === "darwin") {
    configDir = path.join(
      os.homedir(),
      "Library",
      "Application Support",
      "Claude",
    );
    configFile = path.join(configDir, "claude_desktop_config.json");
  } else {
    console.error("Claude desktop is only supported on Windows and macOS.");
    process.exit(1);
  }
  if (!fs.existsSync(configDir)) {
    console.error(
      `Claude desktop config directory does not exist: ${configDir}\nPlease make sure the Claude desktop app is installed for your platform.`,
    );
    process.exit(1);
  }
  let config = {};
  if (fs.existsSync(configFile)) {
    try {
      config = JSON.parse(fs.readFileSync(configFile, "utf8"));
    } catch (e) {
      config = {};
    }
  }
  if (!config.mcpServers || typeof config.mcpServers !== "object") {
    config.mcpServers = {};
  }
  config.mcpServers["terminator-mcp-agent"] = getMcpServerEntry();
  fs.writeFileSync(configFile, JSON.stringify(config, null, 2));
  console.log(`Claude configuration saved to ${configFile}`);
}

function buildVSCodeMcpJsonArg(mcpJson) {
  // VS Code expects: --add-mcp "{\"name\":\"...\",...}"
  return `"${JSON.stringify(mcpJson).replace(/"/g, '\\"')}"`;
}

function addToVSCodeConfig() {
  // VS Code CLI-based setup
  console.log("Adding Terminator MCP to VS Code via code CLI...");
  const mcpJson = {
    name: "terminator-mcp-agent",
    command: "npx",
    args: ["-y", "terminator-mcp-agent"],
  };
  const jsonArg = buildVSCodeMcpJsonArg(mcpJson);
  const vscodeCmd = "code";
  try {
    const { spawnSync } = require("child_process");
    const result = spawnSync(`${vscodeCmd} --add-mcp ${jsonArg}`, [], {
      stdio: "inherit",
      shell: true,
    });
    if (result.error) {
      if (result.error.code === "ENOENT") {
        console.error(
          "'code' command not found in PATH. Make sure VS Code CLI is installed and available.",
        );
      } else {
        console.error("Failed to launch VS Code CLI:", result.error.message);
      }
      process.exit(1);
    }
    if (result.status !== 0) {
      console.error(`VS Code CLI exited with code ${result.status}`);
      process.exit(1);
    }
    console.log("Successfully added Terminator MCP to VS Code.");
  } catch (e) {
    console.error("Failed to add MCP to VS Code:", e.message);
    process.exit(1);
  }
}

function addToVSCodeInsidersConfig() {
  // VS Code Insiders CLI-based setup
  console.log(
    "Adding Terminator MCP to VS Code Insiders via code-insiders CLI...",
  );
  const mcpJson = {
    name: "terminator-mcp-agent",
    command: "npx",
    args: ["-y", "terminator-mcp-agent"],
  };
  const jsonArg = buildVSCodeMcpJsonArg(mcpJson);
  const codeInsidersCmd = "code-insiders";
  try {
    const { spawnSync } = require("child_process");
    const result = spawnSync(`${codeInsidersCmd} --add-mcp ${jsonArg}`, [], {
      stdio: "inherit",
      shell: true,
    });
    if (result.error) {
      if (result.error.code === "ENOENT") {
        console.error(
          "'code-insiders' command not found in PATH. Make sure VS Code Insiders CLI is installed and available.",
        );
      } else {
        console.error(
          "Failed to launch VS Code Insiders CLI:",
          result.error.message,
        );
      }
      process.exit(1);
    }
    if (result.status !== 0) {
      console.error(`VS Code Insiders CLI exited with code ${result.status}`);
      process.exit(1);
    }
    console.log("Successfully added Terminator MCP to VS Code Insiders.");
  } catch (e) {
    console.error("Failed to add MCP to VS Code Insiders:", e.message);
    process.exit(1);
  }
}

function addToWindsurfConfig() {
  // Windsurf config: %USERPROFILE%/.codeium/windsurf/mcp_config.json
  const home = os.homedir();
  const configDir = path.join(home, ".codeium", "windsurf");
  const configFile = path.join(configDir, "mcp_config.json");
  if (!fs.existsSync(configDir)) {
    fs.mkdirSync(configDir, { recursive: true });
  }
  let config = {};
  if (fs.existsSync(configFile)) {
    try {
      config = JSON.parse(fs.readFileSync(configFile, "utf8"));
    } catch (e) {
      config = {};
    }
  }
  if (!config.mcpServers || typeof config.mcpServers !== "object") {
    config.mcpServers = {};
  }
  config.mcpServers["terminator-mcp-agent"] = getMcpServerEntry();
  fs.writeFileSync(configFile, JSON.stringify(config, null, 2));
  console.log(`Windsurf configuration saved to ${configFile}`);
}

function addToApp(app) {
  switch ((app || "").toLowerCase()) {
    case "cursor":
      addToCursorConfig();
      break;
    case "claude":
      addToClaudeConfig();
      break;
    case "vscode":
      addToVSCodeConfig();
      break;
    case "insiders":
      addToVSCodeInsidersConfig();
      break;
    case "windsurf":
      addToWindsurfConfig();
      break;
    default:
      console.error("Unknown app: " + app);
      process.exit(1);
  }
}

const argv = process.argv.slice(2);

if (argv.includes("--add-to-app")) {
  const appIndex = argv.indexOf("--add-to-app") + 1;
  const app =
    argv[appIndex] && !argv[appIndex].startsWith("--")
      ? argv[appIndex]
      : undefined;
  if (!app) {
    // Interactive prompt
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });
    console.log("========== Terminator MCP Setup ==========");
    console.log("Which app do you want to configure Terminator MCP for?");
    console.log("");
    console.log("  1. Cursor");
    console.log("  2. Claude");
    console.log("  3. VS Code");
    console.log("  4. VS Code Insiders");
    console.log("  5. Windsurf");
    console.log("");
    rl.question("Enter your choice (1-5): ", (answer) => {
      let selectedApp = null;
      switch (answer.trim()) {
        case "1":
          selectedApp = "cursor";
          break;
        case "2":
          selectedApp = "claude";
          break;
        case "3":
          selectedApp = "vscode";
          break;
        case "4":
          selectedApp = "insiders";
          break;
        case "5":
          selectedApp = "windsurf";
          break;
        default:
          console.error("Invalid choice. Skipping app configuration.");
          rl.close();
          process.exit(1);
      }
      rl.close();
      addToApp(selectedApp);
      process.exit(0);
    });
    return;
  } else {
    addToApp(app);
    process.exit(0);
  }
}

// Default or --start: run the agent
if (argv.length === 0 || argv.includes("--start")) {
  const { pkg, bin, npmDir } = getPlatformInfo();
  let binary;

  // 1. Try local build (for dev)
  const localPath = path.join(__dirname, "npm", npmDir, bin);
  if (fs.existsSync(localPath)) {
    binary = localPath;
  } else {
    // 2. Try installed npm package
    try {
      const pkgPath = require.resolve(`${pkg}/package.json`);
      const binDir = path.dirname(pkgPath);
      binary = path.join(binDir, bin);
    } catch (e) {
      console.error(`Failed to find platform binary: ${pkg}`);
      process.exit(1);
    }
  }

  const child = spawn(binary, [], { stdio: ["pipe", "pipe", "pipe"], shell: true });

  process.stdin.pipe(child.stdin);
  child.stdout.pipe(process.stdout);

  function killProcess(proc) {
    if (!proc) return;
    const pid = proc.pid;
    if (process.platform === "win32") {
      try {
        execSync(`taskkill /PID ${pid} /T /F`);
      } catch (e) {}
    } else {
      try {
        process.kill(-pid, "SIGKILL");
      } catch (e) {}
    }
  }

  let shuttingDown = false;
  function shutdown() {
    if (shuttingDown) return;
    shuttingDown = true;
    if (child && !child.killed) {
      if (child.stdin) child.stdin.end();
      const termTimeout = setTimeout(() => {
        if (!child.killed) {
          if (process.platform === "win32") {
            killProcess(child);
          } else {
            try {
              process.kill(child.pid, "SIGTERM");
            } catch (e) {}
            setTimeout(() => {
              if (!child.killed) killProcess(child);
            }, 2000);
          }
        }
      }, 2000);
      child.on("exit", () => clearTimeout(termTimeout));
    }
    process.exit();
  }

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
  process.on("exit", shutdown);

  child.on("exit", (code) => {
    console.log(`[MCP exited with code ${code}]`);
    process.exit(code);
  });
}
