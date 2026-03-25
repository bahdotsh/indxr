# Dependency Graph

indxr can generate dependency graphs that visualize relationships between files or symbols. Output as DOT (Graphviz), Mermaid, or JSON.

## Usage

```bash
indxr --graph <FORMAT> [OPTIONS]
```

Where `<FORMAT>` is one of: `dot`, `mermaid`, `json`.

## Graph Levels

### File Level (default)

Shows file-to-file import relationships — which files depend on which other files.

```bash
indxr --graph dot
indxr --graph mermaid
```

### Symbol Level

Shows symbol-to-symbol relationships — trait implementations, method calls, type references.

```bash
indxr --graph dot --graph-level symbol
indxr --graph mermaid --graph-level symbol
```

## Scoping

Use `--filter-path` to scope the graph to a subdirectory. Only files/symbols within the scope become root nodes, but their dependencies outside the scope are still included.

```bash
# Graph centered on the parser module
indxr --graph dot --filter-path src/parser

# Graph centered on the MCP module
indxr --graph mermaid --filter-path src/mcp
```

## Depth Limiting

Use `--graph-depth` to limit how many hops from the scoped files are included:

```bash
# Only direct dependencies (1 hop)
indxr --graph dot --filter-path src/mcp --graph-depth 1

# Up to 2 hops
indxr --graph dot --filter-path src/mcp --graph-depth 2
```

Without `--graph-depth`, all reachable nodes are included.

## Output Formats

### DOT (Graphviz)

```bash
indxr --graph dot -o deps.dot
```

Render with Graphviz:

```bash
dot -Tpng deps.dot -o deps.png
dot -Tsvg deps.dot -o deps.svg
```

### Mermaid

```bash
indxr --graph mermaid -o deps.mmd
```

Mermaid diagrams render directly in GitHub Markdown, GitLab, and many documentation tools.

### JSON

```bash
indxr --graph json -o deps.json
```

Structured output with `nodes` and `edges` arrays for programmatic consumption.

## Examples

### Full project file dependency graph

```bash
indxr --graph dot -o deps.dot
dot -Tsvg deps.dot -o deps.svg
```

### Module-scoped Mermaid diagram

```bash
indxr --graph mermaid --filter-path src/parser
```

### Symbol-level graph for a specific module

```bash
indxr --graph dot --graph-level symbol --filter-path src/model
```

### JSON graph for analysis tooling

```bash
indxr --graph json -o deps.json
```

## Combining with Other Options

The `--graph` flag replaces normal index output. Other flags that affect it:

- `--filter-path` — scope the graph root
- `--graph-level` — choose file or symbol granularity
- `--graph-depth` — limit edge hops
- `-o, --output` — write to file instead of stdout
- `-q, --quiet` — suppress progress output
