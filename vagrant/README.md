# Terminator Development VM

This directory contains the Vagrant configuration for setting up a Windows 10 development environment for the Terminator project.

## Prerequisites

- [Vagrant](https://www.vagrantup.com/downloads)
- [VirtualBox](https://www.virtualbox.org/wiki/Downloads)

## Configuration

The VM is configured with:
- Windows 10 as the base OS
- 4GB RAM
- 2 CPU cores
- SSH access (port 2222)
- The entire workspace is synced to `C:/Users/vagrant/terminator` in the VM
- username - vagrant
- password - vagrant

## Usage

1. Start the VM:
   ```bash
   vagrant up
   ```

2. Connect to the VM:
   ```bash
   vagrant ssh
   ```

3. The workspace will be available at `C:/Users/vagrant/terminator` inside the VM

4. To stop the VM:
   ```bash
   vagrant halt
   ```

5. To destroy the VM and start fresh:
   ```bash
   vagrant destroy
   ```

## Remote Desktop (RDP) Connection

You can connect to the VM using Remote Desktop Protocol (RDP) in two ways:

### Quick Connection
Simply run:
```bash
vagrant rdp
```
This will automatically launch your default RDP client with the correct connection details.

### Manual Connection
If you need the current RDP connection details (which may change between VM restarts), run:
```bash
vagrant winrm-config
```

You can then use any RDP client (like Windows Remote Desktop Connection or Remmina) to connect using the details from the command output.

## VS Code / Cursor Remote SSH

To use VS Code or Cursor with Remote SSH:

1. First, add this entry to your SSH config file:

   For Windows (PowerShell):
   ```powershell
   $sshConfig = @"
   Host default
     HostName 127.0.0.1
     User vagrant
     Port 2222
     UserKnownHostsFile /dev/null
     StrictHostKeyChecking no
     LogLevel FATAL
   "@
   Add-Content -Path "$env:USERPROFILE\.ssh\config" -Value $sshConfig
   ```

   For macOS/Linux (Bash):
   ```bash
   cat << 'EOF' >> ~/.ssh/config
   Host default
     HostName 127.0.0.1
     User vagrant
     Port 2222
     UserKnownHostsFile /dev/null
     StrictHostKeyChecking no
     LogLevel FATAL
   EOF
   ```

   You can change the `Host` name from `default` to anything you prefer.

2. Then connect using:
   ```bash
   code --remote ssh-remote+default C:/Users/vagrant/terminator
   ```
   Replace `default` with your chosen hostname if you changed it.

   When prompted for a password, use: `vagrant`

## Development Workflow

1. The entire workspace is synced between your host machine and the VM
2. Changes made on either side will be reflected on the other
3. You can use your preferred IDE on the host machine while running the code in the VM

## GUI Application Launching

When working through SSH or non-interactive sessions, you can use the `gui-launch` function to start GUI applications in the active user session. This is particularly useful when you need to run Windows applications while connected via SSH or VS Code Remote SSH.

### Usage

1. Launch a simple application:
   ```powershell
   gui-launch notepad.exe
   ```

2. Launch an application with arguments:
   ```powershell
   gui-launch "C:\Program Files\SomeApp\app.exe" /flag
   ```

3. Launch Python scripts with GUI components:
   ```powershell
   gui-launch py .\terminator\examples\snipping_tool.py
   ```

4. Launch applications with relative paths:
   ```powershell
   gui-launch .\my_app\app.exe
   ```

5. Launch applications with complex arguments:
   ```powershell
   gui-launch "C:\Program Files\Microsoft VS Code\Code.exe" --new-window "C:\path\to\file.txt"
   ```

### Features
- Automatically resolves relative paths to absolute paths
- Handles commands with spaces and special characters
- Runs applications in the active user session
- Captures and displays command output
- Works with both simple commands and complex command lines with arguments

### Requirements
- PSTools must be installed (automatically handled by the Vagrant setup)
- Must be run from a PowerShell session
- Requires an active user session on the Windows VM

## Notes

- The VM uses WinRM for initial setup and SSH for subsequent connections
- Default credentials are:
  - Username: vagrant
  - Password: vagrant
- SSH is configured to start automatically on boot 