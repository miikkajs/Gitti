# gitti

A fast, lightweight interactive git diff viewer with IntelliJ-style split-pane UI.

![gitti screenshot](assets/screenshot.png)

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
| `j` / `k` | Scroll diff (3 lines) |
| `PgUp` / `PgDn` | Scroll diff (page) |
| `q` | Quit |

## License

MIT - see [LICENSE](LICENSE) file.
