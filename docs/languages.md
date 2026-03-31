# Supported Languages

indxr supports 27 languages across two parsing strategies: tree-sitter for full AST parsing and regex for structural extraction.

## Tree-Sitter Languages (Full AST Parsing)

These languages are parsed using [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammars, providing accurate, syntax-aware extraction.

### Rust

**Extensions:** `.rs`

**Extracts:**
- Functions (`fn`, `pub fn`, `pub(crate) fn`)
- Structs with fields
- Enums with variants
- Traits with method signatures
- Impl blocks with associated methods
- Modules (`mod`)
- Constants (`const`, `static`)
- Type aliases (`type`)
- Doc comments (`///`, `//!`)
- Visibility levels (pub, pub(crate), private)
- Metadata: `#[test]`, `async`, `#[deprecated]`
- Relationships: trait implementations (`impl Trait for Type`)

### Python

**Extensions:** `.py`, `.pyi`

**Extracts:**
- Functions and methods (`def`, `async def`)
- Classes with methods and attributes
- Decorators (`@decorator`)
- Docstrings (triple-quoted strings)
- Import statements (`import`, `from ... import`)
- Module-level constants (UPPER_CASE assignments)
- Metadata: `async`, `@staticmethod`, `@classmethod`

### TypeScript

**Extensions:** `.ts`, `.tsx`, `.mts`, `.cts`

**Extracts:**
- Functions (named, arrow, exported)
- Classes with methods and properties
- Interfaces
- Enums
- Type aliases
- Export statements
- JSDoc comments (`/** */`)
- Metadata: `async`, `export`, `abstract`
- Relationships: `implements`, `extends`

### JavaScript

**Extensions:** `.js`, `.jsx`, `.mjs`, `.cjs`, `.flow`

**Extracts:**
- Functions (named, arrow, exported)
- Classes with methods
- Export statements (named, default)
- Const declarations
- JSDoc comments (`/** */`)
- Metadata: `async`, `export`

### Go

**Extensions:** `.go`

**Extracts:**
- Functions
- Methods (with receiver types, e.g., `func (s *Server) Handle(...)`)
- Structs with fields
- Interfaces with method signatures
- Constants (`const`)
- Go doc comments (`//` preceding declarations)
- Visibility: exported (capitalized) vs unexported

### Java

**Extensions:** `.java`

**Extracts:**
- Classes (including inner classes)
- Interfaces
- Enums
- Methods and constructors
- Fields
- Annotations (`@Override`, `@Deprecated`, etc.)
- Javadoc comments (`/** */`)
- Visibility: public, protected, private, package-private
- Metadata: `static`, `abstract`, `final`, `synchronized`
- Relationships: `implements`, `extends`

### C

**Extensions:** `.c`, `.h`

**Extracts:**
- Functions with signatures
- Structs with fields
- Enums with variants
- Typedefs
- `#include` directives
- `#define` macros
- Doc comments (`/** */`, `///`)
- Visibility: all treated as public in headers

### C++

**Extensions:** `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`

**Extracts:**
- Everything from C, plus:
- Classes with methods and fields
- Namespaces
- Templates
- Access specifiers (public, protected, private)
- Constructors and destructors
- Virtual methods
- Relationships: inheritance (`:` base classes)

## Regex Languages (Structural Extraction)

These languages use pattern matching for lightweight structural extraction. Less precise than tree-sitter but effective for config, schema, markup, and additional programming languages.

### Shell

**Extensions:** `.sh`, `.bash`, `.zsh`
**Recognized filenames:** `.bashrc`, `.zshrc`, `.bash_profile`, `.profile`

**Extracts:**
- Function definitions (`function name()` and `name()`)
- Export statements (`export VAR=value`)
- Aliases (`alias name=...`)
- Source imports (`source file`, `. file`)

### TOML

**Extensions:** `.toml`

**Extracts:**
- Section headers (`[section]`, `[section.subsection]`)
- Key-value pairs
- Special handling: Cargo.toml dependency extraction (`[dependencies]`, `[dev-dependencies]`)

### YAML

**Extensions:** `.yml`, `.yaml`

**Extracts:**
- Top-level keys
- Special handling: docker-compose service detection

### JSON

**Extensions:** `.json`, `.jsonc`

**Extracts:**
- Top-level keys
- Special handling: package.json dependency extraction

### SQL

**Extensions:** `.sql`

**Extracts:**
- Table definitions (`CREATE TABLE`) with column names
- Views (`CREATE VIEW`)
- Indexes (`CREATE INDEX`)
- Functions/procedures (`CREATE FUNCTION`, `CREATE PROCEDURE`)
- Types (`CREATE TYPE`)

### Markdown

**Extensions:** `.md`, `.markdown`

**Extracts:**
- Heading hierarchy (`#`, `##`, `###`, etc.)

### Protobuf

**Extensions:** `.proto`

**Extracts:**
- Messages with fields
- Services with RPCs
- Enums with values

### GraphQL

**Extensions:** `.graphql`, `.gql`

**Extracts:**
- Types with fields
- Interfaces
- Enums
- Queries
- Mutations
- Subscriptions

### Ruby

**Extensions:** `.rb`, `.rake`, `.gemspec`, `.podspec`
**Recognized filenames:** `Gemfile`, `Rakefile`

**Extracts:**
- Classes and modules
- Methods (`def`)
- Constants
- `require` / `require_relative` imports

### Kotlin

**Extensions:** `.kt`, `.kts`

**Extracts:**
- Classes, objects, interfaces
- Functions (`fun`)
- Properties (`val`, `var`)
- Enums
- Type aliases

### Swift

**Extensions:** `.swift`

**Extracts:**
- Classes, structs, enums, protocols
- Functions (`func`)
- Properties (`let`, `var`)
- Extensions
- Type aliases

### C#

**Extensions:** `.cs`

**Extracts:**
- Classes, structs, interfaces, enums
- Methods
- Properties
- Namespaces
- `using` directives

### Objective-C

**Extensions:** `.m`, `.mm`

**Extracts:**
- Interfaces (`@interface`)
- Implementations (`@implementation`)
- Protocols (`@protocol`)
- Methods (`-` and `+`)
- Properties (`@property`)
- `#import` directives

### XML

**Extensions:** `.xml`, `.plist`, `.svg`, `.xib`, `.storyboard`

**Extracts:**
- Root element
- Top-level elements with attributes

### HTML

**Extensions:** `.html`, `.htm`

**Extracts:**
- Document structure elements
- Script and style blocks

### CSS

**Extensions:** `.css`

**Extracts:**
- Selectors (class, ID, element)
- Media queries
- Keyframe animations
- CSS variables (`--custom-property`)

### Gradle

**Extensions:** `.gradle`, `.gradle.kts`
**Recognized filenames:** `build.gradle`, `settings.gradle`, `build.gradle.kts`, `settings.gradle.kts`

**Extracts:**
- Plugin applications
- Dependency declarations
- Task definitions

### CMake

**Extensions:** `.cmake`
**Recognized filenames:** `CMakeLists.txt`

**Extracts:**
- Function/macro definitions
- Project declarations
- Target definitions (`add_executable`, `add_library`)

### Properties

**Extensions:** `.properties`

**Extracts:**
- Key-value pairs
- Section comments

## Language Detection

Languages are detected by file extension. indxr only processes files with recognized extensions and skips all others. Binary files and files exceeding `--max-file-size` (default 512 KB) are also skipped.

## Filtering by Language

Use `-l` / `--languages` to restrict indexing to specific languages:

```bash
# Only Rust
indxr -l rust

# Rust and Python
indxr -l rust,python

# All config files
indxr -l toml,yaml,json
```

Language names are case-insensitive.
