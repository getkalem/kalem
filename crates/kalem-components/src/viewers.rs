//! The components as viewers of the plugin host (feature `viewers`).

use std::sync::{Arc, OnceLock};

use kalem_script::viewer::{ComponentViewer, VIEWER_LIMITS};
use kalem_script::{Host, Limits};

use crate::Component;

/// A manifest's `opens`: the extensions of the files it shows.
pub fn opens(manifest: &serde_json::Value) -> Vec<String> {
    manifest["opens"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A manifest's `limits` (`memory_mb`, `time_ms`), else a viewer's: a
/// workbook viewer may ask for more memory than the default (wasm_todo
/// W6), whether it is installed or built in.
pub fn limits(manifest: &serde_json::Value) -> Limits {
    Limits {
        memory: manifest["limits"]["memory_mb"]
            .as_u64()
            .map_or(VIEWER_LIMITS.memory, |mb| (mb as usize) << 20),
        time: manifest["limits"]["time_ms"]
            .as_u64()
            .map_or(VIEWER_LIMITS.time, std::time::Duration::from_millis),
    }
}

impl Component {
    /// Its manifest, read.
    pub fn manifest_json(&self) -> serde_json::Value {
        serde_json::from_str(self.manifest).unwrap_or_default()
    }

    /// The viewer it is to Kalem on `host`: named by the last part of its
    /// ID (`org.kalem.xlsx` is `xlsx`, the viewers' registry's name),
    /// with its manifest's name, extensions and limits.
    pub fn viewer(&'static self, host: Arc<Host>) -> ComponentViewer {
        let m = self.manifest_json();
        ComponentViewer::embedded(
            host,
            move || self.wasm(),
            self.id.rsplit('.').next().unwrap_or(self.id),
            m["name"].as_str().unwrap_or(self.id),
            &opens(&m),
            limits(&m),
        )
    }
}

/// The built-in component `id` (`org.kalem.xlsx`) as a viewer, on a host
/// of the process's own: for tests and tools. Each call gives the same
/// viewer, compiled once. The host caches nothing on disk unless
/// `KALEM_COMPONENT_CACHE` names a folder, where processes then share
/// what one of them compiled: CI runs every test in a process of its own
/// (docs/ci_todo.md, C4), and compiling the workbook's component in each
/// took the runners' cores from the other tests.
pub fn viewer(id: &str) -> Option<Arc<ComponentViewer>> {
    static HOST: OnceLock<Option<Arc<Host>>> = OnceLock::new();
    static VIEWERS: OnceLock<Vec<Arc<ComponentViewer>>> = OnceLock::new();
    let all = VIEWERS.get_or_init(|| {
        let Some(host) = HOST
            .get_or_init(|| {
                let cache = std::env::var_os("KALEM_COMPONENT_CACHE").map(std::path::PathBuf::from);
                Host::new(cache).ok().map(Arc::new)
            })
            .clone()
        else {
            return Vec::new();
        };
        crate::components()
            .iter()
            .map(|c| Arc::new(c.viewer(host.clone())))
            .collect()
    });
    crate::components()
        .iter()
        .position(|c| c.id == id)
        .and_then(|i| all.get(i).cloned())
}

#[cfg(test)]
mod tests {
    use kalem_script::viewer::VIEWER_LIMITS;

    #[test]
    fn a_manifest_sets_a_viewers_limits() {
        let m = serde_json::json!({ "limits": { "memory_mb": 3072 } });
        let l = super::limits(&m);
        assert_eq!(l.memory, 3072 << 20);
        assert_eq!(l.time, VIEWER_LIMITS.time);
        let none = super::limits(&serde_json::json!({}));
        assert_eq!(
            (none.memory, none.time),
            (VIEWER_LIMITS.memory, VIEWER_LIMITS.time)
        );
        let opens = super::opens(&serde_json::json!({ "opens": [".pdf"] }));
        assert_eq!(opens, [".pdf"]);
    }
}
