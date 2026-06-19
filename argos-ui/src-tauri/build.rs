use std::path::Path;

fn main() {
    // The frontend dist directory must exist for tauri-build to validate
    // tauri.conf.json. In a fresh clone `npm run build` has not run yet, so we
    // create a minimal placeholder. It is overwritten by the real build.
    let dist = Path::new("../dist");
    if !dist.exists() {
        std::fs::create_dir_all(dist).expect("failed to create dist placeholder dir");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><html lang=\"en\"><body>ArgOS UI placeholder</body></html>",
        )
        .expect("failed to write dist placeholder");
    }
    tauri_build::build();
}
