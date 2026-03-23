# Filtering & Scoped Output

Instead of indexing everything, indxr lets you scope output to exactly what you need. Filters can be combined for precise queries.

## Path Filtering

Restrict output to files under a specific directory:

```bash
indxr --filter-path src/parser
indxr --filter-path tests
indxr --filter-path src/model
```

The path is matched as a prefix against relative file paths. Only files whose path starts with the given prefix are included.

## Language Filtering

Index only specific languages:

```bash
# Single language
indxr -l rust

# Multiple languages
indxr -l rust,python,typescript

# Config files only
indxr -l toml,yaml,json
```

Language names are case-insensitive. See [Supported Languages](languages.md) for the full list.

## Declaration Kind

Filter to a specific kind of declaration:

```bash
indxr --kind function
indxr --kind struct
indxr --kind class
indxr --kind trait
indxr --kind enum
indxr --kind interface
indxr --kind module
indxr --kind constant
indxr --kind method
indxr --kind impl
indxr --kind type
indxr --kind namespace
indxr --kind macro
indxr --kind table
indxr --kind service
indxr --kind message
indxr --kind rpc
```

This filters the output to only show declarations of the specified kind. The full list of kinds depends on the languages being indexed — for example, `table` applies to SQL files, `service`/`message`/`rpc` to Protobuf, and `namespace` to C++.

## Symbol Search

Find a specific symbol by name (case-insensitive substring match):

```bash
# Find anything with "parse" in the name
indxr --symbol parse

# Find the Cache type
indxr --symbol Cache

# Find all "new" constructors
indxr --symbol new
```

## Visibility

Show only public declarations:

```bash
indxr --public-only
```

This filters out private and crate-internal declarations. Useful for generating API references or when agents only need to know the public surface.

## Combining Filters

All filters can be combined:

```bash
# Public functions in src/parser
indxr --filter-path src/parser --kind function --public-only

# All structs named "Config" in Rust files
indxr -l rust --kind struct --symbol Config

# Public API of the model layer
indxr --filter-path src/model --public-only

# Find all async functions
indxr --kind function --symbol async
```

## File Discovery Options

Control which files are even considered for indexing:

```bash
# Limit directory depth
indxr --max-depth 3

# Skip large files (default is 512 KB)
indxr --max-file-size 256

# Exclude patterns
indxr -e "*/tests/*" -e "*/vendor/*" -e "*.generated.*"

# Include gitignored files
indxr --no-gitignore
```

## Output Control

Fine-tune what sections appear in the output:

```bash
# Skip imports section
indxr --omit-imports

# Skip directory tree
indxr --omit-tree

# Both — just declarations
indxr --omit-imports --omit-tree
```

## Practical Examples

### Agent orientation for a subsystem

```bash
indxr --filter-path src/parser --public-only --max-tokens 4000
```

### API reference generation

```bash
indxr --public-only -d signatures -o API.md
```

### Find all test functions

```bash
indxr --kind function --symbol test
```

### Scope to backend in a monorepo

```bash
indxr --filter-path packages/backend -l rust,python
```

### Minimal index for small context windows

```bash
indxr --public-only --omit-imports --omit-tree --max-tokens 2000
```
