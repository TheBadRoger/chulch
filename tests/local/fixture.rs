use anyhow::Result;
use ring::digest::{SHA1_FOR_LEGACY_USE_ONLY, digest};
use serde_json::{Value, json};
use std::io::Write;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

pub(super) struct Fixture {
    pub(super) _dir: TempDir,
    pub(super) source: PathBuf,
    pub(super) root: PathBuf,
    pub(super) manifest: PathBuf,
    pub(super) recipe: Value,
}

pub(super) fn write(base: &Path, path: &str, data: &[u8]) -> Result<()> {
    let file = base.join(path);
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(file, data)?;
    Ok(())
}

pub(super) fn identity(contents: &[u8]) -> Value {
    let sha1: String = digest(&SHA1_FOR_LEGACY_USE_ONLY, contents)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"sha1":sha1,"size":contents.len()})
}

impl Fixture {
    pub(super) fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("local install");
        let root = dir.path().join("data");
        let manifest = dir.path().join("fixture.json");
        let index = br#"{"objects":{}}"#;
        let mut index_meta = identity(index);
        index_meta["id"] = json!("5");
        let vanilla = json!({"id":"1.20.1","javaVersion":{"majorVersion":17},
            "downloads":{"client":identity(b"client")},"assetIndex":index_meta,"libraries":[
            {"name":"skip:linux:1","rules":[{"action":"allow","os":{"name":"linux"}}]}]});
        write(&source, "vanilla.json", &serde_json::to_vec(&vanilla)?)?;
        write(&source, "client.jar", b"client")?;
        write(&source, "assets/indexes/5.json", index)?;
        let loader = json!({"inheritsFrom":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries":[{"name":"net.fabricmc:fabric-loader:0.16.9","rules":null}]});
        write(&source, "fabric.json", &serde_json::to_vec(&loader)?)?;
        write(
            &source,
            "libraries/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar",
            b"loader",
        )?;
        write(&source, "a.jar", b"mod")?;
        write(&source, "b.jar", b"mod")?;
        write(&source, "options.txt", b"initial")?;
        let recipe = json!({"schemaVersion":1,"platform":"windows-x64","javaMajor":17,"minecraft":"1.20.1",
            "fabric":"0.16.9","libraries":"libraries","assets":"assets","clientJar":"client.jar",
            "clientMetadata":"vanilla.json","loaderMetadata":"fabric.json","packFiles":[
                {"role":"mod","source":"a.jar","target":"mods/a.jar"},
                {"role":"mod","source":"b.jar","target":"mods/b.jar"},
                {"role":"template","source":"options.txt","target":"options.txt"}]});
        fs::write(&manifest, serde_json::to_vec(&recipe)?)?;
        Ok(Self {
            _dir: dir,
            source,
            root,
            manifest,
            recipe,
        })
    }

    pub(super) fn install(&self, id: &str) -> Result<()> {
        mcdocker::import(&self.root, id, &self.manifest, &self.source)
    }

    pub(super) fn blobs(&self) -> Result<Vec<PathBuf>> {
        let mut blobs = vec![];
        for prefix in fs::read_dir(self.root.join("store/blobs/sha256"))? {
            for file in fs::read_dir(prefix?.path())? {
                blobs.push(file?.path());
            }
        }
        Ok(blobs)
    }
}

pub(super) fn pack_zip(
    path: &Path,
    manifest_name: &str,
    manifest: &Value,
    files: &[(&str, &[u8])],
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(fs::File::create(path)?);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(manifest_name, options)?;
    zip.write_all(&serde_json::to_vec(manifest)?)?;
    for &(name, bytes) in files {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}
