//! Plain command lines: a subcommand, `--name value` options, `--flag` switches, and the rest
//! as positional arguments.

use std::collections::HashMap;

pub struct Args {
    pub command: String,
    pub positional: Vec<String>,
    options: HashMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    /// `switches` are the options that take no value.
    pub fn parse(switches: &[&str]) -> Self {
        Self::from(std::env::args().skip(1).collect(), switches)
    }

    pub fn from(raw: Vec<String>, switches: &[&str]) -> Self {
        let mut it = raw.into_iter();
        let command = it.next().unwrap_or_default();
        let (mut positional, mut options, mut flags) = (Vec::new(), HashMap::new(), Vec::new());
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                if let Some((k, v)) = name.split_once('=') {
                    options.insert(k.to_string(), v.to_string());
                } else if switches.contains(&name) {
                    flags.push(name.to_string());
                } else if let Some(v) = it.next() {
                    options.insert(name.to_string(), v);
                } else {
                    flags.push(name.to_string());
                }
            } else {
                positional.push(a);
            }
        }
        Self {
            command,
            positional,
            options,
            flags,
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    pub fn num(&self, name: &str, default: f64) -> f64 {
        match self.get(name) {
            Some(v) => v.parse().unwrap_or_else(|_| {
                eprintln!("--{name} wants a number, not {v:?}");
                std::process::exit(2)
            }),
            None => default,
        }
    }
}
