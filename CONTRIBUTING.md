# Contributing to ThukiWin

Thank you for your interest in contributing! This guide will walk you through everything you need to get started.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Development Setup](#development-setup)
- [Running Tests](#running-tests)
- [Code Style](#code-style)
- [Submitting a Pull Request](#submitting-a-pull-request)
- [Good First Issues](#good-first-issues)

---

## Prerequisites

You'll need the following tools installed before you can build ThukiWin:

### Required

**Bun:** JavaScript runtime and package manager

```powershell
powershell -c "irm bun.sh/install.ps1 | iex"
```

**Rust:** required for the Tauri backend

Install via [rustup.rs](https://rustup.rs)

After installation, restart your terminal. ThukiWin builds against stable Rust.

Running the coverage suite (required before submitting a PR) also needs the nightly toolchain with `llvm-tools`:

```powershell
rustup toolchain install nightly --component llvm-tools
```

**Microsoft Visual Studio Build Tools 2022:** required for compiling the Rust backend on Windows

1. Download from [visualstudio.microsoft.com](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Install the **C++ build tools** workload with **MSVC v143** and **Windows 10/11 SDK**

**Windows:** ThukiWin is a Windows application. It uses Win32 APIs (`SetWindowsHookExW`, GDI `BitBlt`, `SendInput`, clipboard APIs) for hotkey detection, screen capture, and context capture.

### Optional

**Ollama:** required at runtime for AI inference

- Install via [ollama.com](https://ollama.com) or `winget install Ollama.Ollama`

---

## Development Setup

1. **Fork and clone the repository**

   ```powershell
   git clone https://github.com/ayzekhdawy/thukiwin.git
   cd thukiwin
   ```

2. **Install frontend dependencies**

   ```powershell
   bun install
   ```

3. **Set up your AI backend**

   Make sure Ollama is running and you have a model pulled:

   ```powershell
   ollama pull gemini-3-flash-preview
   ```

   ThukiWin connects to `http://127.0.0.1:11434` by default.

4. **Configure environment** (optional)

   ```powershell
   copy .env.example .env
   ```

   Edit `.env` to customize quote display behavior or the system prompt. See [docs/configurations.md](docs/configurations.md) for all available options.

5. **Launch the app**

   ```powershell
   bun run dev
   ```

   No special permissions needed on Windows. The global hotkey and screen capture work out of the box.

---

## Running Tests

**100% code coverage is mandatory.** All new or modified code must maintain 100% coverage across lines, functions, branches, and statements. PRs that drop below 100% will not be merged.

### Frontend tests (Vitest + React Testing Library)

```powershell
bun run test              # Run all frontend tests
bun run test:watch        # Watch mode
bun run test:coverage     # Run with coverage report
```

Coverage output is in `coverage/`. Open `coverage/index.html` in a browser for a visual breakdown.

### Backend tests (Cargo)

```powershell
bun run test:backend           # Run all Rust tests
bun run test:backend:coverage  # Run with 100% line coverage enforcement
```

### Run everything

```powershell
bun run test:all           # Both frontend and backend tests
bun run test:all:coverage  # Both with coverage enforcement
```

### Full validation gate

Before submitting a PR, run the full validation suite:

```powershell
bun run validate-build
```

This runs lint, format check, typecheck, and build in sequence. All must pass with zero warnings and zero errors.

---

## Code Style

**Formatting and linting are enforced by CI.** To avoid failed PR checks, run these locally before pushing:

```powershell
bun run format   # Auto-format TypeScript/CSS (Prettier) and Rust (cargo fmt)
bun run lint     # ESLint + cargo clippy
```

Key style rules:
- TypeScript: enforced by ESLint with `@eslint-react` rules
- Rust: enforced by `cargo clippy -- -D warnings` (warnings are errors)
- No `console.log` or debug output in committed code

---

## Submitting a Pull Request

1. **Create a branch** from `main`

   ```powershell
   git checkout -b feat/your-feature-name
   ```

2. **Make your changes** following the code style guidelines above

3. **Write or update tests** to maintain 100% coverage

4. **Run the validation suite**

   ```powershell
   bun run test:all:coverage
   bun run validate-build
   ```

5. **Commit your changes** using [Conventional Commits](https://www.conventionalcommits.org/) format:

   ```
   <type>: <short description>
   ```

   Common types: `feat` (new feature), `fix` (bug fix), `docs` (documentation), `refactor`, `test`, `chore`. Keep the subject line under 72 characters.

6. **Open a PR** against `main` and fill out the PR template fully

7. **Respond to review feedback:** maintainers aim to review within a few days

### PR Guidelines

- Keep PRs focused on a single change. Large, multi-concern PRs are harder to review and slower to merge.
- If you're fixing a bug, include a test that would have caught the bug.
- If you're adding a feature, document it in `docs/configurations.md` if it's configurable.
- Link any related issues in the PR description.

---

## Good First Issues

New to the codebase? Look for issues tagged [`good first issue`](https://github.com/ayzekhdawy/thukiwin/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22) on GitHub. These are scoped to be approachable without deep knowledge of the full system.

If you have a question or want to discuss an approach before writing code, open an issue or start a discussion; we're happy to help.