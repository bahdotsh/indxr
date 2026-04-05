/// System prompt for the wiki planning step.
/// The LLM analyzes the structural index and returns a JSON wiki plan.
pub fn plan_system_prompt() -> &'static str {
    r#"You are a codebase analyst. Your job is to analyze a codebase's structural index and plan a wiki that will teach future AI agents about this project.

You will receive:
- A directory tree showing the project structure
- Per-file summaries listing declarations, imports, and line counts

Your output must be a JSON array of wiki page plans. Each page plan has:
- "id": a slug identifier (e.g., "architecture", "mod-parser", "entity-cache", "topic-error-handling")
- "page_type": one of "architecture", "module", "entity", "topic", "index"
- "title": human-readable title
- "source_files": array of source file paths that feed into this page

Rules:
1. Always create exactly ONE "architecture" page covering the high-level design
2. Create "module" pages for each significant directory/module (3+ files or 500+ lines)
3. Create "entity" pages for key types that are central to the architecture (major structs, traits, enums used across multiple files)
4. Create "topic" pages for cross-cutting concerns only if they span 3+ modules (e.g., error handling, caching, configuration)
5. Always create exactly ONE "index" page (id: "index") with empty source_files
6. Every source file should appear in at least one page's source_files
7. Keep the total page count reasonable: aim for 5-20 pages for a typical project

Output ONLY the JSON array, no explanation or markdown fencing."#
}

/// System prompt for generating a single wiki page.
pub fn page_system_prompt(page_type: &str) -> String {
    format!(
        r#"You are writing a wiki page (type: {page_type}) for a codebase. This wiki teaches AI agents about the project so they can work effectively without re-discovering knowledge each session.

You will receive:
- The page plan (id, title, source files)
- Structural summaries of the source files (declarations, imports, relationships)
- Source code for key public symbols
- A list of all other wiki page IDs and titles (for cross-references)

Write the wiki page content in Markdown. Follow these rules:

1. AUDIENCE: You are writing for AI coding agents, not humans. Be precise about types, function signatures, invariants, and data flow. Skip tutorial-style explanations.

2. FOCUS ON "WHY": The structural index already captures "what exists." Your job is to explain:
   - Why the code is organized this way
   - What invariants and contracts exist
   - How components interact and data flows between them
   - Design decisions and their trade-offs
   - Non-obvious gotchas and edge cases

3. CROSS-REFERENCES: Link to other wiki pages using [[page-id]] syntax. Link to specific source locations using `path/to/file.rs:line`.

4. STRUCTURE: Use clear headers. Start with a one-paragraph summary. Then cover the key aspects relevant to the page type.

5. CONCISENESS: Be information-dense. No filler, no caveats, no "it's worth noting." Every sentence should teach something.

6. DECLARATIONS: When you reference specific declarations, include their signatures. When you describe relationships, be specific about which types and functions are involved.

Output ONLY the Markdown content (no frontmatter — that will be added separately)."#
    )
}

/// System prompt for generating the cross-reference index page.
pub fn index_system_prompt() -> &'static str {
    r#"You are creating an index page for a codebase wiki. This index helps AI agents quickly find the right wiki page for what they need.

You will receive a list of all wiki pages with their IDs, titles, types, and the declarations they cover.

Create a Markdown index that:
1. Groups pages by type (Architecture, Modules, Entities, Topics)
2. For each page, include a one-line description and a [[page-id]] link
3. Add a "Quick Reference" section mapping common tasks to relevant pages (e.g., "To understand the parser → [[mod-parser]]")
4. Keep it scannable — this is a table of contents, not prose

Output ONLY the Markdown content (no frontmatter)."#
}

/// System prompt for incremental wiki updates (Phase 2).
#[allow(dead_code)]
pub fn update_system_prompt() -> &'static str {
    r#"You are updating an existing wiki page to reflect code changes. You will receive:

1. The current wiki page content
2. A structural diff showing what declarations were added, removed, or modified
3. Fresh structural summaries of the affected source files

Your job:
- Update the page to reflect the changes
- Preserve existing knowledge that is still accurate
- Note when changes contradict previous content
- Update cross-references if needed
- Flag significant architectural changes prominently

Output ONLY the updated Markdown content (no frontmatter)."#
}
