use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::page::{PageType, WikiPage};

/// Lightweight manifest entry for fast lookups without parsing all pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub page_type: PageType,
    pub file: String,
    pub source_files: Vec<String>,
}

/// Wiki manifest — persisted as manifest.yaml in the wiki root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiManifest {
    pub version: u32,
    /// Git commit hash the wiki was last generated/updated at.
    pub generated_at_ref: String,
    /// ISO 8601 timestamp.
    pub generated_at: String,
    pub pages: Vec<ManifestEntry>,
}

impl Default for WikiManifest {
    fn default() -> Self {
        Self {
            version: 1,
            generated_at_ref: String::new(),
            generated_at: String::new(),
            pages: Vec::new(),
        }
    }
}

/// On-disk wiki store — a directory of markdown pages plus a manifest.
pub struct WikiStore {
    pub root: PathBuf,
    pub manifest: WikiManifest,
    pub pages: Vec<WikiPage>,
}

impl WikiStore {
    /// Create a new empty store at the given directory.
    pub fn new(wiki_dir: &Path) -> Self {
        Self {
            root: wiki_dir.to_path_buf(),
            manifest: WikiManifest::default(),
            pages: Vec::new(),
        }
    }

    /// Load an existing wiki from disk.
    pub fn load(wiki_dir: &Path) -> Result<Self> {
        let manifest_path = wiki_dir.join("manifest.yaml");
        let manifest: WikiManifest = if manifest_path.exists() {
            let text =
                fs::read_to_string(&manifest_path).context("Failed to read wiki manifest")?;
            serde_yaml::from_str(&text).context("Failed to parse wiki manifest")?
        } else {
            WikiManifest::default()
        };

        let mut pages = Vec::new();
        for entry in &manifest.pages {
            let page_path = wiki_dir.join(&entry.file);
            if page_path.exists() {
                let text = fs::read_to_string(&page_path)
                    .with_context(|| format!("Failed to read wiki page: {}", entry.file))?;
                match WikiPage::parse(&text) {
                    Ok(page) => pages.push(page),
                    Err(e) => eprintln!(
                        "Warning: skipping malformed wiki page {}: {}",
                        entry.file, e
                    ),
                }
            }
        }

        Ok(Self {
            root: wiki_dir.to_path_buf(),
            manifest,
            pages,
        })
    }

    /// Save the wiki to disk — writes all pages and the manifest.
    pub fn save(&self) -> Result<()> {
        // Create subdirectories
        for subdir in &["modules", "entities", "topics"] {
            fs::create_dir_all(self.root.join(subdir))?;
        }

        // Write each page
        for page in &self.pages {
            let rel_path = page_relative_path(page);
            let full_path = self.root.join(&rel_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let rendered = page.render()?;
            fs::write(&full_path, rendered)?;
        }

        // Build and write manifest
        let manifest = WikiManifest {
            version: 1,
            generated_at_ref: self.manifest.generated_at_ref.clone(),
            generated_at: self.manifest.generated_at.clone(),
            pages: self
                .pages
                .iter()
                .map(|p| ManifestEntry {
                    id: p.frontmatter.id.clone(),
                    page_type: p.frontmatter.page_type.clone(),
                    file: page_relative_path(p),
                    source_files: p.frontmatter.source_files.clone(),
                })
                .collect(),
        };

        let yaml = serde_yaml::to_string(&manifest)?;
        fs::write(self.root.join("manifest.yaml"), yaml)?;

        Ok(())
    }

    /// Find a page by ID.
    #[allow(dead_code)]
    pub fn get_page(&self, id: &str) -> Option<&WikiPage> {
        self.pages.iter().find(|p| p.frontmatter.id == id)
    }

    /// Find all pages whose source_files overlap with the given path.
    #[allow(dead_code)]
    pub fn pages_covering_file(&self, path: &str) -> Vec<&WikiPage> {
        self.pages
            .iter()
            .filter(|p| p.frontmatter.source_files.iter().any(|sf| sf == path))
            .collect()
    }

    /// Insert or update a page (matched by ID).
    pub fn upsert_page(&mut self, page: WikiPage) {
        if let Some(existing) = self
            .pages
            .iter_mut()
            .find(|p| p.frontmatter.id == page.frontmatter.id)
        {
            *existing = page;
        } else {
            self.pages.push(page);
        }
    }
}

/// Compute the relative path within the wiki dir for a page.
fn page_relative_path(page: &WikiPage) -> String {
    let filename = page.filename();
    match page.frontmatter.page_type.subdir() {
        Some(subdir) => format!("{}/{}", subdir, filename),
        None => filename,
    }
}
