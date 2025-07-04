# Specifies the base image for the container. Using Windows Server Core 2022.
FROM mcr.microsoft.com/windows/servercore:ltsc2022

# Sets the default shell to PowerShell for subsequent RUN instructions.
SHELL ["powershell", "-Command", "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue';"]

# Installs Chocolatey, a package manager for Windows.
RUN Set-ExecutionPolicy Bypass -Scope Process -Force; `
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; `
    iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))

# Refreshes environment variables to include Chocolatey in the PATH.
RUN refreshenv

# Installs essential development tools using Chocolatey.
# - Visual Studio 2022 Build Tools: For MSVC toolchain required by Rust on Windows.
# - Rust: The Rust programming language and its toolchain.
# - Node.js LTS: Long-term support version of Node.js for JavaScript runtime.
# - Python: Python programming language.
RUN choco install -y visualstudio2022buildtools --package-parameters '--workload Microsoft.VisualStudio.Workload.VCTools --includeRecommended'; `
    choco install -y rust; `
    choco install -y nodejs-lts; `
    choco install -y python

# Refreshes environment variables to include paths of newly installed tools.
RUN refreshenv

# Adds Cargo's bin directory to the system's PATH. This ensures that tools installed via `cargo install` are available.
RUN $user_profile = [System.Environment]::GetFolderPath('UserProfile'); `
    $cargo_path = Join-Path $user_profile '.cargo\bin'; `
    $current_path = [System.Environment]::GetEnvironmentVariable('PATH', 'Machine'); `
    if (-not ($current_path -like "*$cargo_path*")) { `
        [System.Environment]::SetEnvironmentVariable('PATH', "$($current_path);$($cargo_path)", 'Machine'); `
    }

# Refreshes environment variables again to include Rust's path.
RUN refreshenv

# Initializes and updates the Rust toolchain to the stable version.
RUN rustup default stable; `
    rustup update

# Sets the working directory inside the container.
WORKDIR /app

# Copies all project files from the host to the container's working directory.
# For faster builds, this could be optimized to first copy only dependency-defining files (like Cargo.toml, package.json),
# install dependencies, and then copy the source code.
COPY . .

# Builds the entire Rust workspace in release mode.
# This step will compile all crates, including the main library and agents.
RUN cargo build --release

# Sets the default command to execute when the container starts.
# Opens a PowerShell session for interactive use.
CMD ["powershell"]
