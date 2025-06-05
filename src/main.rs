use clap::{Parser, Subcommand};
use rusqlite::{Connection, Result};
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR")]
    pub docset_dir: Option<std::path::PathBuf>,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    ListDocsets,
    Search { docset: String, query: Vec<String> },
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

fn search_docset(docset_path: &Path, query: &str) -> Result<bool> {
    let db_path = docset_path.join("Contents/Resources/docSet.dsidx");
    let conn = Connection::open(&db_path)?;

    let like_pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT name, type, path FROM searchIndex WHERE name LIKE ?1 ORDER BY name COLLATE NOCASE",
    )?;

    let mut rows = stmt.query([like_pattern])?;
    let mut any = false;

    while let Some(row) = rows.next()? {
        let name: String = row.get(0)?;
        let typ: String = row.get(1)?;
        let path: String = row.get(2)?;
        if !any {
            println!("Results for \"{}\":", query);
            any = true;
        }
        println!("{:30} {:10} {}", name, typ, path);
    }

    if !any {
        println!("No results found for \"{}\"", query);
    }

    Ok(any)
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
            if query.is_empty() {
                eprintln!("No search query given.");
                std::process::exit(1);
            }
            let base = zeal_docsets_dir(&cli.docset_dir).expect("Docsets directory not found");
            let docset_path = base.join(format!("{}.docset", docset));
            if !docset_path.exists() {
                eprintln!("Docset '{}' not found at {:?}", docset, docset_path);
                std::process::exit(1);
            }
            let query = query.join(" ");
            match search_docset(&docset_path, &query) {
                Ok(true) => {}
                Ok(false) => println!("No results found for '{}' in docset '{}'", query, docset),
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
