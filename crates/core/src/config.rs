//! JSON settings files, read and written the way the Python app did: missing or
//! broken files give defaults, writes go through a temporary file and a rename.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

/// Read a JSON object; anything missing or unreadable gives an empty object.
pub fn load(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Write a JSON value atomically, indented like Python's `json.dumps(indent=1)`.
pub fn save(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut text = to_pretty(value);
    text.push('\n');
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

fn to_pretty(value: &Value) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(value, &mut ser).expect("JSON values always serialize");
    String::from_utf8(buf).expect("serde_json writes UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_and_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/x.json");
        assert!(load(&p).is_empty());
        save(&p, &json!({"color": "#ff2800", "targets": ["mouse"]})).unwrap();
        let m = load(&p);
        assert_eq!(m["color"], "#ff2800");
        assert!(fs::read_to_string(&p).unwrap().starts_with("{\n \"color\""));
        fs::write(&p, "not json").unwrap();
        assert!(load(&p).is_empty());
    }
}
