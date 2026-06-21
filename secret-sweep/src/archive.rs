use crate::manifest::Manifest;
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

pub fn create_inner_zip(manifest: &Manifest, files: &[(String, String)]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let manifest_json = serde_json::to_vec_pretty(manifest)?;
        zip.start_file("manifest.json", options)?;
        zip.write_all(&manifest_json)?;

        for (archive_path, source_path) in files {
            let mut source = File::open(source_path)
                .with_context(|| format!("open {}", source_path))?;
            zip.start_file(archive_path, options)?;
            std::io::copy(&mut source, &mut zip)?;
        }

        zip.finish()?;
    }
    Ok(buf)
}

pub fn extract_inner_zip(data: &[u8], out_dir: &Path) -> Result<Manifest> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).context("invalid inner zip")?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name.contains("..") {
            bail!("invalid archive path: {name}");
        }
        let out_path = out_dir.join(&name);
        if name.ends_with('/') {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut file, &mut out)?;
    }

    let manifest_path = out_dir.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path).context("missing manifest.json")?;
    let manifest: Manifest = serde_json::from_str(&manifest_text).context("parse manifest.json")?;
    Ok(manifest)
}
