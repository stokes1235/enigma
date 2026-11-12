mod decode_map;

use std::io::Read;

/// Serialize a decoded prost message to pretty JSON, dropping null/empty noise.
fn dump<T: serde::Serialize>(msg: &T) -> String {
    let value = serde_json::to_value(msg).expect("serialize decoded message");
    serde_json::to_string_pretty(&prune(value)).expect("pretty json")
}

fn prune(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter_map(|(k, v)| match prune(v) {
                    serde_json::Value::Null => None,
                    v => Some((k, v)),
                })
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(prune).collect())
        }
        other => other,
    }
}

fn message_name(filename: &str) -> Option<(String, bool)> {
    let stem = filename.strip_suffix(".bin")?;
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() < 5 {
        return None;
    }
    let name = parts[4..].join("_");
    let is_raw = name.starts_with("cmd_");
    Some((name, is_raw))
}

/// Older capture tools write unknown commands as unsigned 16-bit numbers.
/// Resolve their original request/reply names from the protocol's own mapping,
/// including historical aliases such as RougeRougeSelectDropCmd.
fn raw_message_name(name: &str) -> Option<&'static str> {
    let (wire, direction) = name.strip_prefix("cmd_")?.rsplit_once('_')?;
    let wire: u16 = wire.parse().ok()?;
    let signed = wire as i16 as i32;
    let mut original = None;
    let mut in_commands = false;
    for line in include_str!("../../protocol/include/_.rs").lines() {
        let line = line.trim();
        if line == "pub enum CmdId {" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line == "}" {
            break;
        }
        if let Some(names) = line.strip_prefix("/// / Original: ") {
            original = Some(names);
        } else if let Some((_, value)) = line.split_once(" = ") {
            if value.trim_end_matches(',').parse::<i32>().ok() == Some(signed) {
                let names = original?;
                return match direction {
                    "out" => names.split(" / ").next(),
                    "in" => names.split(" / ").nth(1).or(Some(names)),
                    _ => None,
                };
            }
            original = None;
        }
    }
    None
}

fn process(path: &std::path::Path, filter: &str) -> std::io::Result<()> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        entries.sort();
        for entry in entries {
            if entry.is_file() {
                process(&entry, filter)?;
            } else if entry.is_dir() {
                process(&entry, filter)?;
            }
        }
        return Ok(());
    }

    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let Some((mut name, mut is_raw)) = message_name(&filename) else {
        return Ok(());
    };
    if is_raw && let Some(resolved) = raw_message_name(&name) {
        name = resolved.to_owned();
        is_raw = false;
    }
    if !name.is_empty() && !filter.is_empty() && !name.contains(filter) {
        return Ok(());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;

    let seq = filename.split('_').nth(3).unwrap_or("??????").to_string();
    if is_raw {
        println!("=== {seq} {name} ({}B) raw hex:", bytes.len());
        dump_hex(&bytes);
        return Ok(());
    }
    match decode_map::decode_named(&name, &bytes) {
        Some(json) => {
            println!("=== {seq} {name} ({}B)\n{json}", bytes.len())
        }
        None => {
            println!("=== {seq} {name} ({}B) UNKNOWN-TYPE raw:", bytes.len());
            dump_hex(&bytes)
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsigned_commands_resolve_before_filtering() {
        assert_eq!(
            raw_message_name("cmd_39956_out"),
            Some("RougeRoundMoveRequest")
        );
        assert_eq!(
            raw_message_name("cmd_62792_out"),
            Some("RougeSelectDropRequest")
        );
        assert_eq!(
            raw_message_name("cmd_44159_out"),
            Some("RougeRecruitHeroRequest")
        );
        assert_eq!(
            raw_message_name("cmd_62792_in"),
            Some("RougeSelectDropReply")
        );
        assert_eq!(raw_message_name("cmd_99999_out"), None);
    }
}

fn dump_hex(bytes: &[u8]) {
    const W: usize = 16;
    for (i, chunk) in bytes.chunks(W).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("{:08x}  {:<47}  {}", i * W, hex.join(" "), ascii);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: capture_dump [--filter Rouge] <file-or-dir>...");
        std::process::exit(2);
    }
    let mut filter = String::new();
    let mut paths = Vec::new();
    for arg in &args {
        if let Some(f) = arg.strip_prefix("--filter=") {
            filter = f.to_string();
        } else {
            paths.push(arg.clone());
        }
    }
    for path in &paths {
        if let Err(e) = process(std::path::Path::new(path), &filter) {
            eprintln!("{}: {e}", path);
        }
    }
}
