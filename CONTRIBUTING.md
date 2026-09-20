# Contributing to VRChat Organizer

Thank you for your interest in contributing! This project is a pure-Rust rewrite of the VRChat Screenshot Organizer. Below are guidelines to help you get started.

## Getting Started

1. Fork the repository
2. Clone your fork locally
3. Create a new branch for your feature or bugfix
4. Make your changes
5. Test your changes thoroughly
6. Push to your fork
7. Create a Pull Request

## Code Style

- Follow standard [Rust style guidelines](https://doc.rust-lang.org/1.0.0/style/)
- Use `cargo fmt` before committing to ensure consistent formatting
- Run `cargo clippy` to catch common mistakes and improve code quality
- Use meaningful variable and function names
- Add doc comments (`///`) to public functions, structs, and traits
- Include `Result` return types with `anyhow::Context` for error propagation

## Project Structure

The project is a Cargo workspace with three crates:

| Crate | Description |
|---|---|
| `organizer-core` | Core library: metadata extraction (EXIF/PNG text chunks), file organization, undo system |
| `desktop` | CLI binary — simple command-line interface using environment variables |
| `src-tauri` | Tauri v2 desktop application (GUI shell with HTML/JS frontend) |

## Testing

Before submitting a PR, please:

- Run `cargo test --workspace` to verify all tests pass
- Run `cargo build --release` to ensure the project compiles without warnings
- Test the CLI tool:
  ```bash
  VRCHAT_DRY_RUN=1 cargo run -p desktop --release -- ~/Pictures/VRChat/VRChat
  ```
- Test the Tauri GUI:
  ```bash
  cargo build -p app --release
  ./target/release/app
  ```
- Verify no regressions in metadata extraction for both PNG and JPEG files

## Commit Messages

- Use clear, descriptive commit messages
- Start with a capital letter
- Use present tense ("Add feature" not "Added feature")
- Keep commits focused and atomic

## Reporting Issues

When reporting issues, please include:

- A clear description of the problem
- Steps to reproduce
- Expected behavior
- Actual behavior
- Your environment (OS, Rust version with `rustc --version`)
- The image type (PNG/JPEG) if related to metadata extraction

## Pull Request Process

1. Ensure your code compiles without warnings (`cargo build`)
2. Run tests: `cargo test --workspace`
3. Format your code: `cargo fmt`
4. Run clippy: `cargo clippy`
5. Update documentation if you change any public API or behavior
6. Your PR will be reviewed by a maintainer

## Questions?

Feel free to open an issue for any questions or discussions. Thank you for contributing!
