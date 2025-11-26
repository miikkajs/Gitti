# gitti

A fast, lightweight interactive git diff viewer with IntelliJ-style split-pane UI.

## Features

- 🎨 **IntelliJ-style dark theme** with Darcula colors
- 🌈 **Syntax highlighting** - auto-detects language from file extension (Swift, Rust, Python, JS, etc.)
- 📂 **Split-pane UI** - file list on left, diff on right
- ⌨️ **Keyboard navigation** - select files with arrow keys
- 📊 **Smart hunks** - shows only changed lines + 5 lines of context
- ⚡ **Fast** - uses libgit2 directly, no subprocess
- 🔧 **Lightweight** - minimal dependencies

## Installation

```bash
cargo build --release
cargo install --path .
```

## Usage

```bash
gitti                    # Show unstaged changes
gitti --staged           # Show staged changes  
gitti -c HEAD~1          # Compare with commit
gitti -C 10              # 10 lines of context (default: 5)
```

## Controls

| Key | Action |
|-----|--------|
| `↑` / `↓` | Select file |
| `j` / `k` | Scroll diff |
| `PgUp` / `PgDn` | Scroll diff |
| `q` | Quit |

## Screenshot

```
┌─────────────────────┬────────────────────────────────────────┐
│ Changes (3)         │ src/main.rs                            │
├─────────────────────┼────────────────────────────────────────┤
│ ~ src/main.rs       │  10   10 │   use std::io;              │
│ + src/new.rs        │  11   11 │                             │
│ - old_file.rs       │  12      │ - fn old_function() {       │
│                     │      12  │ + fn new_function() {       │
│                     │  13   13 │     println!("Hello");      │
│                     │─────────────────────────────────────── │
│                     │  45   45 │   let x = 1;                │
│                     │  46      │ - let y = 2;                │
│                     │      46  │ + let y = 3;                │
└─────────────────────┴────────────────────────────────────────┘
 ↑↓ Navigate files │ j/k Scroll diff │ q Quit
```

## License

MIT
