//! Embeds the built UI (`src/ui/dist`, pre-gzipped by `src/ui/scripts/gzip.mjs`) into the binary.
//! Without a build, a one-line placeholder page is embedded so `cargo build` always works.
use std::{env, fs, path::Path, path::PathBuf};

fn mime(p: &str) -> &'static str {
    match p.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" | "webmanifest" => "application/json",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out)
        } else {
            out.push(p)
        }
    }
}

fn main() {
    let dist = env::var("TIK_UI_DIST").map(PathBuf::from).unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/dist"));
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-env-changed=TIK_UI_DIST");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut files = Vec::new();
    walk(&dist, &mut files);
    files.sort();
    let mut code = String::from("pub static ASSETS: &[(&str, &str, bool, &[u8])] = &[\n");
    for f in &files {
        let rel = f.strip_prefix(&dist).unwrap().to_string_lossy().replace('\\', "/");
        let Some(orig) = rel.strip_suffix(".gz") else {
            if files.contains(&PathBuf::from(format!("{}.gz", f.display()))) {
                continue; // the .gz twin is served instead
            }
            code.push_str(&format!("    (\"/{rel}\", \"{}\", false, include_bytes!({:?})),\n", mime(&rel), f));
            continue;
        };
        code.push_str(&format!("    (\"/{orig}\", \"{}\", true, include_bytes!({:?})),\n", mime(orig), f));
    }
    if files.is_empty() {
        let ph = out.join("placeholder.html");
        fs::write(&ph, "<!doctype html><title>tikontrol</title><p>UI not built: run <code>npm run build</code> in src/ui, then rebuild.</p>").unwrap();
        code.push_str(&format!("    (\"/index.html\", \"text/html; charset=utf-8\", false, include_bytes!({ph:?})),\n"));
    }
    code.push_str("];\n");
    fs::write(out.join("assets.rs"), code).unwrap();
}
