use std::path::PathBuf;

pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mtui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    pub fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn write(&self, file: &str, content: &str) -> String {
        let path = self.path(file);
        std::fs::write(&path, content).expect("scratch file");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
