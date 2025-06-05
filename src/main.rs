use ansi_term::Colour;
use clap::{Parser, Subcommand};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use rusqlite::{Connection, Result};
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR")]
    pub docset_dir: Option<std::path::PathBuf>,
    #[arg(long, default_value_t = false)]
    pub icons: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    ListDocsets,
    Search { docset: String, query: Vec<String> },
}

fn type_icon(type_: &str) -> String {
    match type_.to_lowercase().as_str() {
        "guide" => Colour::Green.paint("󰗚").to_string(),
        "section" => Colour::Yellow.paint("§").to_string(),
        "function" => Colour::Cyan.paint("ƒ").to_string(),
        "method" => Colour::Blue.paint("m").to_string(),
        "class" => Colour::Purple.paint("🅒").to_string(),
        "struct" | "_struct" => Colour::Red.paint("🅢").to_string(),
        "enum" => Colour::Purple.paint("🄴").to_string(),
        "constant" => Colour::Blue.paint("𝑪").to_string(),
        "property" => Colour::Yellow.paint("").to_string(),
        "macro" => Colour::Cyan.paint("μ").to_string(),
        "interface" => Colour::Purple.paint("🄸").to_string(),
        "typedef" | "type" => Colour::Cyan.paint("𝙏").to_string(),
        "attribute" => Colour::Yellow.paint("󰓹").to_string(),
        "event" => Colour::Cyan.paint("").to_string(),
        "variable" => Colour::Blue.paint("𝚟").to_string(),
        "module" => Colour::Yellow.paint("󰏖").to_string(),
        "constructor" => Colour::Red.paint("").to_string(),
        other => other.to_string(),
    }
}

fn zeal_docsets_dir(override_dir: &Option<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    if let Some(dir) = override_dir {
        return Some(dir.clone());
    }
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir().map(|h| h.join(".local/share/Zeal/Zeal/docsets"))
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| h.join("Library/Application Support/Zeal/Zeal/docsets"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("Zeal").join("Zeal").join("docsets"))
    }
}

fn list_docsets(docsets_dir: &Option<std::path::PathBuf>) -> std::io::Result<Vec<String>> {
    if let Some(dir) = zeal_docsets_dir(docsets_dir) {
        let entries = fs::read_dir(dir)?
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    let path = e.path();
                    if path.is_dir() {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            })
            .collect();
        Ok(entries)
    } else {
        Ok(vec![])
    }
}

fn check_bin(bin: &str) -> Result<(), String> {
    which::which(bin)
        .map(|_| ())
        .map_err(|_| format!("Cannot find binary `{}`", bin))
}

fn search_docset(
    docset_path: &Path,
    query: &str,
    icons: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let db_path = docset_path.join("Contents/Resources/docSet.dsidx");
    let docs_dir = docset_path.join("Contents/Resources/Documents");
    let conn = Connection::open(&db_path)?;

    let mut matches = Vec::new();
    let matcher = SkimMatcherV2::default();

    if query.is_empty() {
        // Fast path: no fuzzy matching
        let mut stmt = conn.prepare("SELECT name, type, path FROM searchIndex")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                0, // score
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        matches = rows
            .map(|r| r.map(|(score, name, typ, path)| (score, name, typ, docs_dir.join(path))))
            .collect::<Result<Vec<_>, _>>()?;
        matches.sort_by(|a, b| a.1.cmp(&b.1));
    } else {
        let mut stmt = conn
            .prepare("SELECT name, type, path FROM searchIndex WHERE name LIKE '%' || ?1 || '%'")?;
        let rows = stmt.query_map([query], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let candidates: Vec<_> = rows.collect::<Result<_, _>>()?;

        // Parallel fuzzy matching
        let mut scored: Vec<_> = candidates
            .par_iter()
            .filter_map(|(name, typ, path)| {
                matcher
                    .fuzzy_match(name, query)
                    .map(|score| (score, name.clone(), typ.clone(), path.clone()))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        // Only join path when actually displaying
        matches = scored
            .into_iter()
            .map(|(score, name, typ, path)| (score, name, typ, docs_dir.join(path)))
            .collect();
    }

    for (_, name, typ, html_path) in &matches {
        if icons {
            println!(
                "{}\t{}\t{}\t{}",
                type_icon(typ),
                name,
                typ,
                html_path.display()
            );
        } else {
            println!("\t{}\t{}\t{}", name, typ, html_path.display());
        }
    }

    Ok(matches.len())
}

fn main() {
    let cli = Cli::parse();

    check_bin("zeal").unwrap_or_else(|e| eprintln!("{}", e));

    match &cli.command {
        Some(Commands::ListDocsets) => match list_docsets(&cli.docset_dir) {
            Ok(docsets) if !docsets.is_empty() => {
                for d in docsets {
                    println!("{}", d);
                }
            }
            Ok(_) => println!("No docsets found."),
            Err(e) => eprintln!("Error listing docsets: {}", e),
        },
        Some(Commands::Search { docset, query }) => {
            let base = zeal_docsets_dir(&cli.docset_dir).expect("Docsets directory not found");
            let docset_path = base.join(format!("{}.docset", docset));
            if !docset_path.exists() {
                eprintln!("Docset '{}' not found at {:?}", docset, docset_path);
                std::process::exit(1);
            }
            let query = query.join(" ");
            match search_docset(&docset_path, &query, cli.icons) {
                Ok(0) => println!("No results found for '{}' in docset '{}'", query, docset),
                Ok(_) => {} // results already printed line-by-line
                Err(e) => {
                    eprintln!("Error searching docset '{}': {}", docset, e);
                    std::process::exit(1);
                }
            }
        }
        None => {
            println!("No command provided.");
            std::process::exit(1);
        }
    }
}
