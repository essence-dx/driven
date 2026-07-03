use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

pub struct DrivenDxConfig {
    pub workspace_root: PathBuf,
    pub cache_dir: PathBuf,
    pub sr_dir: PathBuf,
    pub receipts_dir: PathBuf,
}

impl DrivenDxConfig {
    pub fn load() -> Self {
        let config = ::dx_config::DxConfig::load(
            &std::env::current_dir().unwrap_or_default(),
        )
        .unwrap_or_default();

        let ws = config.workspace.root.clone();
        let cache = config.paths.cache.clone();
        let sr = cache.parent().map(|p| p.join("serializer")).unwrap_or_else(|| ws.join(".dx").join("serializer"));
        let receipts = ws.join(".dx").join("receipts").join("driven");

        Self {
            workspace_root: ws,
            cache_dir: cache,
            sr_dir: sr,
            receipts_dir: receipts,
        }
    }

    pub fn sr_path(&self, name: &str) -> PathBuf {
        self.sr_dir.join(format!("{}.sr", name))
    }

    pub fn read_status(&self, name: &str) -> Option<HashMap<String, String>> {
        let sr_path = self.sr_path(name);
        dx_config::read_machine_or_sr(&sr_path)
    }

    pub fn machine_path(&self, name: &str) -> PathBuf {
        self.sr_dir.join(format!("{}.machine", name))
    }

    /// Write a .sr file using DX LLM key=value format.
    pub fn write_sr(&self, name: &str, entries: &[(&str, &str)]) -> std::io::Result<()> {
        let path = self.sr_path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf: Vec<u8> = Vec::new();
        for (key, value) in entries {
            write!(buf, "{key}=")?;
            Self::write_llm_value(&mut buf, value)?;
            buf.push(b'\n');
        }
        let tmp = path.with_extension("sr.tmp");
        std::fs::write(&tmp, &buf)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    fn write_llm_value(buf: &mut Vec<u8>, value: &str) -> std::io::Result<()> {
        if value.is_empty() {
            buf.extend_from_slice(b"\"\"");
            return Ok(());
        }
        let needs_quoting = value.contains(|c: char| {
            c.is_ascii_whitespace() || c == '"' || c == '[' || c == ']' || c == '=' || c == '#'
        });
        if needs_quoting {
            buf.push(b'"');
            for c in value.chars() {
                if c == '"' || c == '\\' {
                    buf.push(b'\\');
                }
                let mut tmp = [0u8; 4];
                buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
            }
            buf.push(b'"');
        } else {
            buf.extend_from_slice(value.as_bytes());
        }
        Ok(())
    }
}
