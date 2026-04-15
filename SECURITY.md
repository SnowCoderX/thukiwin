# Security Policy

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report vulnerabilities privately via [GitHub Security Advisories](https://github.com/ayzekhdawy/thukiwin/security/advisories/new). This keeps the details confidential until a fix is ready.

We will acknowledge your report within **48 hours** and aim to release a fix within **14 days** for critical issues, depending on severity and complexity.

## Scope

ThukiWin runs entirely on your local machine. There is no server, no cloud backend, and no telemetry. The attack surface is limited to:

- The Tauri IPC boundary between the frontend and Rust backend
- The Windows keyboard hook and clipboard simulation for context capture (`windows_activator.rs`)
- Screen capture via GDI BitBlt through the Tauri command layer (`windows_screenshot.rs`)
- The local SQLite database storing conversation history
- Image processing via the `image` crate

## Supported Versions

We support the latest release only. Please verify you are on the latest version before reporting.