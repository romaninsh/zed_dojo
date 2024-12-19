use anyhow::{Context, Result};
use mlua::prelude::*;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rand::seq::SliceRandom;
use regex::{Captures, Regex};
use std::{fs, path::Path, sync::mpsc::channel};

#[derive(Debug, Clone)]
struct Level {
    path: Option<String>,
    conditions: Vec<String>,
    completion_regex: Regex,
    instructions: String,
    captures: Option<Vec<(String, Regex)>>,
}

fn main() -> Result<()> {
    // let content_path = "content/name.txt";
    let tutorial_path = "tutorial.txt";

    // Read the level file (content/welcome.txt)
    // let level_data = fs::read_to_string(content_path).context("Failed to read level file")?;

    let lua = Lua::new();

    loop {
        // Load a random level
        let level = load_random_level(&lua, "content")?;

        // Write instructions to tutorial.txt
        fs::write(tutorial_path, &level.instructions).context("Failed to write to tutorial.txt")?;
        println!(
            "Instructions from {} written to {}. Edit the file to complete the level.",
            level.path.as_ref().unwrap_or(&"<unknown>".to_string()),
            tutorial_path
        );

        // Create a channel to receive the events
        let (tx, rx) = channel();

        // Automatically select the best implementation for your platform
        let mut watcher = RecommendedWatcher::new(tx, Config::default())
            .context("Failed to create file watcher")?;

        // Add a path to be watched
        watcher
            .watch(Path::new(tutorial_path), RecursiveMode::NonRecursive)
            .context("Failed to watch tutorial.txt")?;

        loop {
            match rx.recv() {
                Ok(Ok(event)) => {
                    if matches!(event.kind, EventKind::Modify(_)) {
                        let file_content = fs::read_to_string(tutorial_path)
                            .context("Failed to read tutorial.txt")?;

                        // if file is empty, skip
                        if file_content.trim().is_empty() {
                            println!("Empty file, skipping.");
                            break;
                        }

                        let matched = file_content
                            .lines()
                            .any(|line| level.completion_regex.is_match(line));

                        if matched {
                            println!("Completion pattern matched! Level complete.");

                            if let Some(captures) = &level.captures {
                                for (var_name, regex) in captures {
                                    if let Some(capture) =
                                        find_captures_in_lines(&regex, file_content.lines())
                                    {
                                        // Use capture.get(1) to access the first capturing group
                                        if let Some(matched) = capture.get(1) {
                                            println!("Captured {}: {}", var_name, matched.as_str());

                                            lua.globals()
                                                .set(var_name.clone(), matched.as_str())?;
                                        } else {
                                            println!(
                                                "Capture for {} exists, but no groups found.",
                                                var_name
                                            );
                                        }
                                    } else {
                                        println!(
                                            "No match for capture {}, regexp: {:?}.",
                                            var_name, regex
                                        );
                                    }
                                }
                            }
                            break;
                        } else {
                            println!("Pattern not matched yet. Keep trying!");
                        }
                    }
                }
                Ok(Err(e)) => println!("Watch error: {:?}", e),
                Err(e) => println!("Channel receive error: {:?}", e),
            }
        }
    }
}

fn parse_level_file(lua: &Lua, level_data: &str) -> Result<Level> {
    let parts: Vec<&str> = level_data.split("---").collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid level file format");
    }

    let metadata = parts[0];
    let instructions = parts[1].trim().to_string();

    // Parse metadata with Lua
    LuaErrorContext::context(
        lua.load(metadata).exec(),
        "Failed to parse metadata with Lua",
    )?;

    let completion_regex_str: String = lua.globals().get("completion")?;
    let completion_regex = Regex::new(&completion_regex_str).context("Invalid regex pattern")?;

    let captures = extract_captures(&lua).ok();

    Ok(Level {
        path: None,
        conditions: vec![],
        completion_regex,
        instructions,
        captures,
    })
}

fn extract_captures(lua: &Lua) -> Result<Vec<(String, Regex)>> {
    let table = lua.globals().get::<LuaTable>("capture")?;

    let mut captures = vec![];

    for pair in table.pairs::<String, String>() {
        let (var_name, regex_str) =
            LuaErrorContext::context(pair, "Failed to read capture pair from Lua table")?;
        let regex = Regex::new(&regex_str)
            .context(format!("Invalid regex pattern for capture '{}'", var_name))?;
        captures.push((var_name, regex));
    }

    Ok(captures)
}

fn find_captures_in_lines<'a>(
    regex: &Regex,
    lines: impl Iterator<Item = &'a str>,
) -> Option<Captures<'a>> {
    lines.filter_map(|line| regex.captures(line)).next()
}

fn load_random_level(lua: &Lua, content_folder: &str) -> Result<Level> {
    let mut errors = Vec::new();
    let mut levels = Vec::new();

    // List all files in the content folder
    let entries = fs::read_dir(content_folder)
        .context(format!("Failed to read content folder: {}", content_folder))?;

    for entry in entries {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        // Skip non-file entries
        if !path.is_file() {
            continue;
        }

        // Attempt to parse the level file
        match fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {:?}", path))
            .and_then(|data| parse_level_file(lua, &data))
        {
            Ok(mut level) => {
                level.path = Some(path.to_string_lossy().to_string());
                levels.push(level);
            }
            Err(err) => errors.push((path, err)),
        }
    }

    // If no levels loaded successfully, report errors
    if levels.is_empty() {
        eprintln!("Failed to load any levels. Errors:");
        for (path, err) in &errors {
            eprintln!("{:?}: {}", path, err);
        }
        anyhow::bail!("No valid levels found in content folder");
    }

    // Randomly select a level
    let chosen_level = levels
        .choose(&mut rand::thread_rng())
        .context("Failed to randomly select a level")?;

    Ok(chosen_level.clone()) // Assuming Level implements Clone
}
