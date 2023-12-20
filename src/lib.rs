use colored::*;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    error::Error,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use toml_edit::{Array, Decor, Document, InlineTable, Item, Table, Value};

/// Type alias for shorter return types.
pub type Res<T> = Result<T, Box<dyn Error>>;

/// A TOML entry. Generic to support both `Item` and `Value` entries.
struct Entry<T> {
    key: String,
    value: T,
    decor: Decor,
}

#[derive(StructOpt, Debug)]
pub struct Opt {
    /// List of .toml files to format.
    #[structopt(name = "FILE", parse(from_os_str))]
    pub files: Vec<PathBuf>,

    /// Only check the formatting, returns an error if the file is not formatted.
    /// If not provide the files will be overritten.
    #[structopt(short, long)]
    pub check: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    /// Important keys in non-inline tables.
    /// Will be sorted first, then any non-important keys will be
    /// sorted lexicographically.
    #[serde(default)]
    pub keys: Vec<String>,

    /// Important keys in inline tables.
    /// Will be sorted first, then any non-important keys will be
    /// sorted lexicographically.
    #[serde(default)]
    pub inline_keys: Vec<String>,

    /// Does it sort arrays of strings ?
    /// In case of mixed types, string will be ordered first, then
    /// other values in original order.
    #[serde(default)]
    pub sort_string_arrays: bool,
}

const CONFIG_FILE: &'static str = "toml-sort.toml";

impl Config {
    pub fn read_from_file() -> Option<Config> {
        let mut path: PathBuf = std::env::current_dir().ok()?;
        let filename = Path::new(CONFIG_FILE);

        loop {
            path.push(filename);

            if path.is_file() {
                let text = std::fs::read_to_string(&path).ok()?;
                let config: Self = toml::from_str(&text).ok()?;
                return Some(config);
            }

            if !(path.pop() && path.pop()) {
                // remove file && remove parent
                return None;
            }
        }
    }
}

pub struct ProcessedConfig {
    /// Important keys in non-inline tables.
    /// Will be sorted first, then any non-important keys will be
    /// sorted lexicographically.
    pub keys: BTreeMap<String, usize>,

    /// Important keys in non-inline tables.
    /// Will be sorted first, then any non-important keys will be
    /// sorted lexicographically.
    pub inline_keys: BTreeMap<String, usize>,

    /// Does it sort arrays of strings ?
    /// In case of mixed types, string will be ordered first, then
    /// other values in original order.
    pub sort_string_arrays: bool,
}

impl From<Config> for ProcessedConfig {
    fn from(x: Config) -> Self {
        let mut res = Self {
            keys: BTreeMap::new(),
            inline_keys: BTreeMap::new(),
            sort_string_arrays: x.sort_string_arrays,
        };

        for (i, key) in x.keys.iter().enumerate() {
            res.keys.insert(key.clone(), i);
        }

        for (i, key) in x.inline_keys.iter().enumerate() {
            res.inline_keys.insert(key.clone(), i);
        }

        res
    }
}

fn absolute_path(path: impl AsRef<Path>) -> Res<String> {
    Ok(std::fs::canonicalize(&path)?.to_string_lossy().to_string())
}

impl ProcessedConfig {
    /// Process the provided file.
    pub fn process_file(&self, path: impl AsRef<Path>, check: bool) -> Res<()> {
        let absolute_path = absolute_path(&path)?;
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!(
                "Error while reading file \"{}\" : {}",
                absolute_path,
                e.to_string().red()
            );
            std::process::exit(3);
        });

        let doc = text.parse::<Document>()?;
        let trailing = doc.trailing().trim_end();

        let output_table = self.format_table(&doc)?;
        let mut output_doc: Document = output_table.into();
        output_doc.set_trailing(trailing); // Insert back trailing content (comments).
        let output_text = format!("{}\n", output_doc.to_string().trim());

        if check {
            if text != output_text {
                eprintln!("Check fails : {}", absolute_path.red());
                std::process::exit(2);
            } else {
                println!("Check succeed: {}", absolute_path.green());
            }
        } else {
            if text != output_text {
                let mut file = File::create(&path)?;
                file.write_all(output_text.as_bytes())?;
                file.flush()?;
                println!("Overwritten: {}", absolute_path.blue());
            } else {
                println!("Unchanged: {}", absolute_path.green());
            }
        }

        Ok(())
    }

    /// Format a `Table`.
    /// Consider empty lines as "sections" and will not sort accross sections.
    /// Comments at the start of the section will stay at the start, while
    /// comments attached to any other line will stay attached to that line.
    fn format_table(&self, table: &Table) -> Res<Table> {
        let mut formated_table = Table::new();
        formated_table.set_implicit(true); // avoid empty `[dotted.keys]`
        let prefix = table.decor().prefix().unwrap_or("");
        let suffix = table.decor().suffix().unwrap_or("");
        formated_table.decor_mut().set_prefix(prefix);
        formated_table.decor_mut().set_suffix(suffix);

        let mut section_decor = Decor::default();
        let mut section = Vec::<Entry<Item>>::new();

        let sort = |x: &Entry<Item>, y: &Entry<Item>| {
            let xord = self.keys.get(&x.key);
            let yord = self.keys.get(&y.key);

            match (xord, yord) {
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (Some(x), Some(y)) => x.cmp(y),
                (None, None) => x.key.cmp(&y.key),
            }
        };

        // Iterate over all original entries.
        for (i, (key, item)) in table.iter().enumerate() {
            let mut key_decor = table.key_decor(key).unwrap().clone();

            // First entry can be decored (prefix).
            // In that case we want to keep that decoration at the start of the section.
            if i == 0 {
                if let Some(prefix) = key_decor.prefix() {
                    if !prefix.is_empty() {
                        section_decor.set_prefix(prefix);
                        key_decor.set_prefix("".to_string());
                    }
                }
            }
            // Later entries can contain a new-line prefix decor.
            // It means it is a new section, and sorting must not cross
            // section boundaries.
            else if let Some(prefix) = key_decor.prefix() {
                if prefix.starts_with("\n") {
                    // Sort keys and insert them.
                    section.sort_by(sort);

                    for (i, mut entry) in section.into_iter().enumerate() {
                        // Add section prefix.
                        if i == 0 {
                            if let Some(prefix) = section_decor.prefix() {
                                entry.decor.set_prefix(prefix);
                            }
                        }

                        formated_table.insert(&entry.key, entry.value);
                        *formated_table.key_decor_mut(&entry.key).unwrap() = entry.decor;
                    }

                    // Cleanup for next sections.
                    section = Vec::new();
                    section_decor = Decor::default();
                    section_decor.set_prefix(prefix);
                    key_decor.set_prefix("".to_string());
                }
            }

            // Remove any trailing newline in decor suffix.
            if let Some(suffix) = key_decor.suffix().map(|x| x.to_owned()) {
                key_decor.set_suffix(suffix.trim_end_matches('\n'));
            }

            // Format inner item.
            let new_item = match item {
                Item::None => Item::None,
                Item::Value(inner) => Item::Value(self.format_value(&inner, false)?),
                Item::Table(inner) => Item::Table(self.format_table(inner)?),
                // TODO : Doesn't seem we have any of those.
                Item::ArrayOfTables(inner) => Item::ArrayOfTables(inner.clone()),
            };

            section.push(Entry {
                key: key.to_string(),
                value: new_item,
                decor: key_decor,
            });
        }

        // End of entries, we insert remaining section.
        section.sort_by(sort);

        for (i, mut entry) in section.into_iter().enumerate() {
            // Add section prefix.
            if i == 0 {
                if let Some(prefix) = section_decor.prefix() {
                    entry.decor.set_prefix(prefix);
                }
            }

            formated_table.insert(&entry.key, entry.value);
            *formated_table.key_decor_mut(&entry.key).unwrap() = entry.decor;
        }

        Ok(formated_table)
    }

    /// Format inline tables `{ key = value, key = value }`.
    /// TOML doesn't seem to support inline comments, so we just override entries decors
    /// to respect proper spaces.
    pub fn format_inline_table(&self, table: &InlineTable, last: bool) -> Res<InlineTable> {
        let mut formated_table = InlineTable::new();
        if last {
            formated_table.decor_mut().set_suffix(" ");
        }

        let mut entries = Vec::<Entry<Value>>::new();

        let sort = |x: &Entry<Value>, y: &Entry<Value>| {
            let xord = self.inline_keys.get(&x.key);
            let yord = self.inline_keys.get(&y.key);

            match (xord, yord) {
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (Some(x), Some(y)) => x.cmp(y),
                (None, None) => x.key.cmp(&y.key),
            }
        };

        for (key, value) in table.iter() {
            let mut key_decor = table.key_decor(key).unwrap().clone();

            // Trim decor.
            key_decor.set_prefix(" ");
            key_decor.set_suffix(" ");

            let new_value = value.clone();

            entries.push(Entry {
                key: key.to_string(),
                value: new_value,
                decor: key_decor,
            });
        }

        entries.sort_by(sort);

        let len = entries.len();
        for (i, entry) in entries.into_iter().enumerate() {
            let new_value = self.format_value(&entry.value, i + 1 == len)?;

            formated_table.insert(&entry.key, new_value);
            *formated_table.key_decor_mut(&entry.key).unwrap() = entry.decor;
        }

        Ok(formated_table)
    }

    /// Format a `Value`.
    pub fn format_value(&self, value: &Value, last: bool) -> Res<Value> {
        Ok(match value {
            Value::Array(inner) => Value::Array(self.format_array(inner, last)?),
            Value::InlineTable(inner) => Value::InlineTable(self.format_inline_table(inner, last)?),
            v => {
                let mut v = v.clone();

                // Keep existing prefix/suffix with correct format.
                let prefix = v.decor().prefix().map(|x| x.trim()).unwrap_or("");

                let prefix = if prefix.is_empty() {
                    prefix.to_string()
                } else {
                    format!(" {}", prefix)
                };

                let suffix = v.decor().suffix().map(|x| x.trim()).unwrap_or("");

                let suffix = if suffix.is_empty() {
                    suffix.to_string()
                } else {
                    format!(" {}", suffix)
                };

                // Convert simple '...' to "..."
                // Doesn't modify strings starting with multiple ' as they
                // Doesn't modify strings containing \ or "
                // could be multiline literals.
                let mut display = v.clone().decorated("", "").to_string();
                if display.starts_with("'")
                    && !display.starts_with("''")
                    && display.find(&['\\', '"'][..]).is_none()
                {
                    if let Some(s) = display.strip_prefix("'") {
                        display = s.to_string();
                    }

                    if let Some(s) = display.strip_suffix("'") {
                        display = s.to_string();
                    }

                    v = display.into();
                }

                // Handle surrounding spaces.
                if last {
                    v.decorated(&format!("{} ", prefix), &format!("{} ", suffix))
                } else {
                    v.decorated(&format!("{} ", prefix), &format!("{}", suffix))
                }
            }
        })
    }

    /// Format an `Array`.
    /// Detect if the array is inline or multi-line, and format it accordingly.
    /// Support comments in multi-line arrays.
    /// With config `sort_string_arrays` the array String entries will be sorted, otherwise will be kept
    /// as is.
    fn format_array(&self, array: &Array, last: bool) -> Res<Array> {
        let mut values: Vec<_> = array.iter().cloned().collect();

        if self.sort_string_arrays {
            values.sort_by(|x, y| match (x, y) {
                (Value::String(x), Value::String(y)) => x.value().cmp(y.value()),
                (Value::String(_), _) => Ordering::Less,
                (_, Value::String(_)) => Ordering::Greater,
                (_, _) => Ordering::Equal,
            });
        }

        let mut new_array = Array::new();

        for value in values.into_iter() {
            new_array.push_formatted(value);
        }

        // Multiline array
        if array.trailing().starts_with("\n") {
            new_array.set_trailing(array.trailing());
            new_array.set_trailing_comma(true);

            for value in new_array.iter_mut() {
                let prefix = value
                    .decor()
                    .prefix()
                    .unwrap_or("")
                    .trim_matches(&[' ', '\t', '\n'][..]);

                let prefix = if !prefix.is_empty() {
                    format!("\n\t{}\n\t", prefix)
                } else {
                    "\n\t".to_string()
                };

                let suffix = value
                    .decor()
                    .suffix()
                    .unwrap_or("")
                    .trim_matches(&[' ', '\t', '\n'][..]);

                let formatted_value = self.format_value(&value, false)?;
                *value = formatted_value.decorated(&prefix, suffix);
            }
        }
        // Inline array
        else {
            new_array.set_trailing("");
            new_array.set_trailing_comma(false);

            let len = new_array.len();
            for (i, value) in new_array.iter_mut().enumerate() {
                *value = self.format_value(&value, i + 1 == len)?;
            }
        }

        new_array.decor_mut().set_prefix(" ");
        new_array
            .decor_mut()
            .set_suffix(if last { " " } else { "" });

        Ok(new_array)
    }
}
