//! reading config.toml, and writing changed settings back to it without
//! disturbing the rest of the file (comments included)

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// the starting config, built into the binary. @RUST_DIR@ stands for this
/// crate's folder, where the bell sounds are
const DEFAULT_CONFIG: &str = include_str!("../config.default.toml");

/// config.toml in the same folder as the running binary. --config
/// overrides it
pub fn default_config_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find where the binary is: {e}"))?;
    let dir = exe.parent().ok_or("the binary has no folder")?;
    Ok(dir.join("config.toml"))
}

/// the built-in starting config with the crate folder filled in
pub fn default_config_text() -> String {
    DEFAULT_CONFIG.replace("@RUST_DIR@", env!("CARGO_MANIFEST_DIR"))
}

/// write the starting config to path if there's nothing there yet.
/// true if it was created
pub fn create_if_missing(path: &Path) -> Result<bool, String> {
    if path.exists() {
        return Ok(false);
    }
    fs::write(path, default_config_text())
        .map_err(|e| format!("can't create the config file {}: {e}", path.display()))?;
    Ok(true)
}

/// the loaded config, with typed lookups
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub table: toml::Table,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("can't read the config file {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("problem in {}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        text.parse::<toml::Table>().map(|table| Config { table }).map_err(|e| e.to_string())
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.table.get(key).and_then(|v| v.as_integer())
    }

    /// a whole number or a decimal, as a decimal
    pub fn float(&self, key: &str) -> Option<f64> {
        let v = self.table.get(key)?;
        v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|v| v.as_bool())
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.table.get(key).and_then(|v| v.as_str())
    }

    /// a setting that must be there, as a column count or similar
    pub fn require_usize(&self, key: &str) -> Result<usize, String> {
        self.int(key)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| format!("config needs {key} set to a whole number of 0 or more"))
    }

    pub fn require_bool(&self, key: &str) -> Result<bool, String> {
        self.bool(key).ok_or_else(|| format!("config needs {key} set to true or false"))
    }

    pub fn require_str(&self, key: &str) -> Result<&str, String> {
        self.str(key).ok_or_else(|| format!("config needs {key} set to a \"string\""))
    }

    /// a list of column numbers like `[10, 20]`; missing means empty
    pub fn usize_list(&self, key: &str) -> Result<Vec<usize>, String> {
        let bad = || format!("config needs {key} set to a list of whole numbers, like [10, 20]");
        match self.table.get(key) {
            None => Ok(Vec::new()),
            Some(toml::Value::Array(items)) => items
                .iter()
                .map(|v| v.as_integer().and_then(|i| usize::try_from(i).ok()).ok_or_else(bad))
                .collect(),
            Some(_) => Err(bad()),
        }
    }

    pub fn set_str(&mut self, key: &str, value: &str) {
        self.table.insert(key.into(), toml::Value::String(value.into()));
    }
}

/// a setting value that can be written back to the config
#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Int(i64),
    Bool(bool),
    /// a list of whole numbers, written as [1, 2, 3]
    Ints(Vec<i64>),
}

impl fmt::Display for Setting {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Setting::Int(i) => write!(f, "{i}"),
            Setting::Bool(b) => write!(f, "{b}"),
            Setting::Ints(v) => {
                let items: Vec<String> = v.iter().map(i64::to_string).collect();
                write!(f, "[{}]", items.join(", "))
            }
        }
    }
}

impl From<usize> for Setting {
    fn from(v: usize) -> Self {
        Setting::Int(v as i64)
    }
}

impl From<&[usize]> for Setting {
    fn from(v: &[usize]) -> Self {
        Setting::Ints(v.iter().map(|&i| i as i64).collect())
    }
}

impl From<bool> for Setting {
    fn from(v: bool) -> Self {
        Setting::Bool(v)
    }
}

/// where the value starts and ends in a line like `key = 12 # comment`,
/// if the line sets key to a whole number, true/false or a list of whole
/// numbers on one line
fn value_span(line: &str, key: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix(key)?;
    let after_key = rest.trim_start_matches([' ', '\t']);
    let after_eq = after_key.strip_prefix('=')?;
    let value = after_eq.trim_start_matches([' ', '\t']);
    let start = line.len() - value.len();
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    let len = if digits > 0 {
        digits
    } else if value.starts_with("true") {
        4
    } else if value.starts_with("false") {
        5
    } else if value.starts_with('[') {
        value.find(']')? + 1
    } else {
        return None;
    };
    Some((start, start + len))
}

/// replace the value of key in the config text, or add `key = value` to
/// the end if it isn't there
pub fn replace_setting(text: &str, key: &str, value: Setting) -> String {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some((start, end)) = value_span(line, key) {
            let (start, end) = (offset + start, offset + end);
            return format!("{}{value}{}", &text[..start], &text[end..]);
        }
        offset += line.len();
    }
    let mut text = text.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("{key} = {value}\n"));
    text
}

/// write whole number, true/false or list settings back to the config file,
/// leaving the rest of the file (including comments) untouched
pub fn save_settings(path: &Path, settings: &[(&str, Setting)]) -> std::io::Result<()> {
    let mut text = fs::read_to_string(path)?;
    for (key, value) in settings {
        text = replace_setting(&text, key, value.clone());
    }
    fs::write(path, text)
}

/// write the margin values back to the config file
pub fn save_margins(path: &Path, left: usize, right: usize) -> std::io::Result<()> {
    save_settings(path, &[("left_margin", left.into()), ("right_margin", right.into())])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_WITH_COMMENTS: &str = "\
width = 75
margin_bell = 8 # numbers of spaces from left margin to bell
right_margin = 75 # set the distance to the right margin
left_margin = 0
save_folder = \"/somewhere/\"
";

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn read(path: &Path) -> (String, Config) {
        let text = fs::read_to_string(path).unwrap();
        let cfg = Config::parse(&text).unwrap();
        (text, cfg)
    }

    #[test]
    fn save_bell_setting_as_toml_bool() {
        let (_d, path) = write("bell = true # comment\nwidth = 75\n");
        save_settings(&path, &[("bell", false.into())]).unwrap();
        let (text, cfg) = read(&path);
        assert!(text.contains("bell = false # comment"));
        assert_eq!(cfg.bool("bell"), Some(false));
        save_settings(&path, &[("bell", true.into())]).unwrap();
        assert_eq!(read(&path).1.bool("bell"), Some(true));
    }

    #[test]
    fn save_margins_updates_values_and_keeps_comments() {
        let (_d, path) = write(CONFIG_WITH_COMMENTS);
        save_margins(&path, 10, 62).unwrap();
        let (text, cfg) = read(&path);
        assert!(text.contains("right_margin = 62 # set the distance to the right margin"));
        assert!(text.contains("left_margin = 10\n"));
        assert!(text.contains("margin_bell = 8 # numbers"));
        assert_eq!(cfg.int("left_margin"), Some(10));
        assert_eq!(cfg.int("right_margin"), Some(62));
        assert_eq!(cfg.int("width"), Some(75));
    }

    #[test]
    fn save_margins_adds_missing_keys() {
        let (_d, path) = write("width = 75\nsave_folder = \"x\""); // no trailing newline
        save_margins(&path, 3, 70).unwrap();
        let (_, cfg) = read(&path);
        assert_eq!(cfg.int("left_margin"), Some(3));
        assert_eq!(cfg.int("right_margin"), Some(70));
        assert_eq!(cfg.str("save_folder"), Some("x"));
    }

    #[test]
    fn save_margins_does_not_touch_similar_keys() {
        let (_d, path) = write("left_margin = 0\nright_margin = 75\nold_left_margin = 9\n");
        save_margins(&path, 4, 50).unwrap();
        let (_, cfg) = read(&path);
        assert_eq!(cfg.int("old_left_margin"), Some(9));
        assert_eq!(cfg.int("left_margin"), Some(4));
    }

    #[test]
    fn longer_key_with_same_start_is_left_alone() {
        let text = replace_setting("bell_sound = 1\nbell = true\n", "bell", false.into());
        assert_eq!(text, "bell_sound = 1\nbell = false\n");
    }

    #[test]
    fn save_line_spacing_keeps_comment() {
        let (_d, path) = write("width = 75\nspacing_index = 1 # 1.5 line spacing\n");
        save_settings(&path, &[("spacing_index", 2usize.into())]).unwrap();
        let (text, cfg) = read(&path);
        assert!(text.contains("spacing_index = 2 # 1.5 line spacing"));
        assert_eq!(cfg.int("width"), Some(75));
    }

    #[test]
    fn save_tab_stops_as_a_list() {
        let (_d, path) = write("tab_stops = [] # columns\nwidth = 75\n");
        save_settings(&path, &[("tab_stops", [10usize, 25].as_slice().into())]).unwrap();
        let (text, cfg) = read(&path);
        assert!(text.contains("tab_stops = [10, 25] # columns"), "{text}");
        assert_eq!(cfg.usize_list("tab_stops").unwrap(), vec![10, 25]);
        save_settings(&path, &[("tab_stops", [].as_slice().into())]).unwrap();
        assert_eq!(read(&path).1.usize_list("tab_stops").unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn tab_stops_missing_or_bad() {
        assert!(Config::parse("width = 1").unwrap().usize_list("tab_stops").unwrap().is_empty());
        assert!(Config::parse("tab_stops = 5").unwrap().usize_list("tab_stops").is_err());
        assert!(Config::parse("tab_stops = [-1]").unwrap().usize_list("tab_stops").is_err());
    }

    #[test]
    fn built_in_config_parses_and_finds_the_bell() {
        let cfg = Config::parse(&default_config_text()).unwrap();
        assert!(cfg.int("width").is_some());
        assert!(cfg.int("printer_vendor_id").is_some()); // hex number
        let bell = cfg.str("bell_sound").unwrap();
        assert!(!bell.contains('@'));
        assert!(Path::new(bell).is_file(), "{bell}");
    }

    #[test]
    fn config_created_only_when_missing() {
        let (_d, path) = write("width = 1\n");
        assert!(!create_if_missing(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "width = 1\n");
        fs::remove_file(&path).unwrap();
        assert!(create_if_missing(&path).unwrap());
        assert!(Config::load(&path).unwrap().int("width").is_some());
    }
}
