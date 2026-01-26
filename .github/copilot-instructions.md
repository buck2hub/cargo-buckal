# Copilot instructions for this repository

These instructions are intended for an automated coding agent (such as GitHub Copilot) working in this repository. They describe where to look for important information and how to validate changes.

## 1. Understanding the repository

- Start by reading `README.md` in the repository root. Treat it as the source of truth for:
  - What the project does.
  - Any language, framework, or runtime requirements.
  - Basic build and run instructions.
- Look for common project metadata files to infer the tech stack and tooling:
  - For Rust: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`.
  - For JavaScript/TypeScript: `package.json`, `tsconfig.json`, `vite.config.*`, `webpack.config.*`.
  - For Python: `pyproject.toml`, `requirements.txt`, `setup.cfg`, `tox.ini`.
  - For Java: `pom.xml`, `build.gradle`, `settings.gradle`.
  - For .NET: `*.csproj`, `Directory.Build.props`, `global.json`.
  - For other ecosystems, inspect the main project files in the repo root or top-level subdirectories.

When unsure which stack is in use, list the files in the repository root and look for the recognized manifest files above before making changes.

## 2. Building and running

Before attempting any build or test:

1. Check `README.md` and any `CONTRIBUTING.md` or `docs/` files for explicit setup or build instructions.
2. If there is a language-specific tool:
   - Rust: run `cargo build` to compile. Use `cargo run` to execute the binary if applicable.
   - JavaScript/TypeScript: run `npm install` or `yarn install` once before building or testing.
   - Python: create and activate a virtual environment if documented, then install dependencies with `pip install -r requirements.txt` or `pip install .` as described.
   - Java: use `mvn` or `gradle` according to the build file present.
   - .NET: use `dotnet restore` before `dotnet build` if documented.

Always prefer the exact commands and versions documented in this repository over inferred defaults.

## 3. Testing and validation

- Search the repository for test-related files and scripts:
  - Rust: `cargo test` (unit/doc/integration tests), `cargo clippy` (linter), `cargo fmt` (formatter).
  - JavaScript/TypeScript: `npm test`, `npm run lint`, or similarly named scripts in `package.json`.
  - Python: `pytest`, `tox`, or `python -m unittest`, depending on the configuration files present.
  - Java: `mvn test` or `gradle test`.
  - .NET: `dotnet test` for solutions or projects.
- Before proposing changes, run at least:
  - The main test command documented in the project.
  - Any linter or formatter commands that are clearly configured (for example, `cargo clippy`, `npm run lint`, `flake8`, `black`, `eslint`, `prettier`, or equivalent).

If tests or linters are configured but slow, it is still preferred to run the full suite unless instructions in this repository explicitly recommend a subset.

## 4. Code layout and configuration

- Look for:
  - Source directories such as `src/`, `lib/`, `app/`, `server/`, or `backend/`.
  - Test directories such as `tests/`, `__tests__/`, `spec/`, or `test/`.
  - Configuration files for tooling in the root or a `config/` directory (for example, `clippy.toml`, ESLint, Prettier, Jest, Pytest, or CI configs).
- When making changes:
  - Modify code in the primary source directories rather than build outputs or generated files.
  - Mirror existing patterns (coding style, file naming, and directory structure) instead of inventing new conventions.

## 5. CI and GitHub Actions

- Inspect `.github/workflows/` to understand:
  - Which commands are run on push and pull requests.
  - Which environments or versions are used (for example, Rust, Node, Python, Java, .NET versions).

Before finalizing a change:

- Run locally the same core commands that the workflows use for build, lint, and test, when they are practical to run.
- If a command appears in CI but cannot easily be run locally (for example, due to missing secrets or cloud infrastructure), avoid modifying that part of the system unless necessary, and clearly explain any assumptions.

## 6. Working strategy for Copilot

- Prefer using existing scripts (for example, `npm run <script>`, `make <target>`, `cargo <command>`, or other defined commands) over calling low-level tools directly.
- When adding new code:
  - Follow existing patterns for error handling, logging, and configuration.
  - Add or update tests alongside code changes when tests exist for similar functionality.
- Only introduce new dependencies if clearly justified and consistent with the existing ecosystem of the repository.
- If documentation or configuration in this repository conflicts with these generic instructions, follow the repository’s own documentation.
