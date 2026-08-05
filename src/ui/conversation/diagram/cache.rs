use std::fs;
use std::path::PathBuf;

pub struct MermaidCache {
    root: PathBuf,
}

impl MermaidCache {
    pub fn new(root: PathBuf) -> Self {
        if !root.exists() {
            let _ = fs::create_dir_all(&root);
        }
        Self { root }
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path_for(key);
        fs::read(path).ok()
    }

    pub fn put(&self, key: &str, png: &[u8]) -> std::io::Result<()> {
        let path = self.path_for(key);
        fs::write(path, png)
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        // Include render version so theme/flag changes bust the cache.
        let ver = crate::ui::conversation::diagram::mmdc::RENDER_CACHE_VERSION;
        self.root.join(format!("{ver}-{key}.png"))
    }
}
