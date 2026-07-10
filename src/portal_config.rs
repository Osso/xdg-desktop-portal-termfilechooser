#[cfg(not(test))]
use std::io::Write;
use std::path::{Path, PathBuf};

const FILECHOOSER_KEY: &str = "org.freedesktop.impl.portal.FileChooser";
const BACKEND_NAME: &str = "termfilechooser";

pub(super) fn set_filechooser_backend(content: &str) -> String {
    let mut lines: Vec<String> = content.split_inclusive('\n').map(String::from).collect();
    if !content.is_empty() && !content.ends_with('\n') {
        let final_line = content.rsplit_once('\n').map_or(content, |(_, line)| line);
        if lines.last().is_some_and(|line| line == final_line) {
            lines.pop();
            lines.push(final_line.to_string());
        }
    }

    let Some(section_start) = lines.iter().position(|line| line.trim() == "[preferred]") else {
        if !content.is_empty() && !content.ends_with('\n') {
            lines.push("\n".into());
        }
        lines.push("[preferred]\n".into());
        lines.push(preference_line());
        return lines.concat();
    };

    let section_end = lines
        .iter()
        .enumerate()
        .skip(section_start + 1)
        .find(|(_, line)| {
            let trimmed = line.trim();
            trimmed.starts_with('[') && trimmed.ends_with(']')
        })
        .map_or(lines.len(), |(index, _)| index);

    if let Some(existing) =
        (section_start + 1..section_end).find(|index| is_filechooser_preference(&lines[*index]))
    {
        let newline = if lines[existing].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        lines[existing] = format!("{FILECHOOSER_KEY}={BACKEND_NAME}{newline}");
        return lines.concat();
    }

    let mut insertion = section_end;
    while insertion > section_start + 1 && lines[insertion - 1].trim().is_empty() {
        insertion -= 1;
    }
    lines.insert(insertion, preference_line());
    lines.concat()
}

fn preference_line() -> String {
    format!("{FILECHOOSER_KEY}={BACKEND_NAME}\n")
}

fn is_filechooser_preference(line: &str) -> bool {
    line.split_once('=')
        .is_some_and(|(key, _)| key.trim() == FILECHOOSER_KEY)
}

pub(super) fn find_policy_source(
    user_directory: &Path,
    system_directories: &[PathBuf],
    desktop: &str,
) -> Option<PathBuf> {
    let user_desktop = user_directory.join(format!("{desktop}-portals.conf"));
    if user_desktop.is_file() {
        return Some(user_desktop);
    }
    let user_generic = user_directory.join("portals.conf");
    if user_generic.is_file() {
        return Some(user_generic);
    }
    for directory in system_directories {
        let desktop_policy = directory.join(format!("{desktop}-portals.conf"));
        if desktop_policy.is_file() {
            return Some(desktop_policy);
        }
    }
    for directory in system_directories {
        let generic_policy = directory.join("portals.conf");
        if generic_policy.is_file() {
            return Some(generic_policy);
        }
    }
    None
}

#[cfg(not(test))]
pub(super) fn configure() -> Result<PathBuf, String> {
    let desktop = current_desktop()?;
    let user_directory = dirs::config_dir()
        .ok_or_else(|| "Cannot determine XDG configuration directory".to_string())?
        .join("xdg-desktop-portal");
    let system_directories = system_policy_directories();
    let source = find_policy_source(&user_directory, &system_directories, &desktop)
        .ok_or_else(|| format!("No existing portal policy found for desktop {desktop}"))?;
    let target = if source.starts_with(&user_directory) {
        source.clone()
    } else {
        user_directory.join(format!("{desktop}-portals.conf"))
    };
    let content = std::fs::read_to_string(&source)
        .map_err(|error| format!("Failed to read portal policy {}: {error}", source.display()))?;
    let updated = set_filechooser_backend(&content);

    std::fs::create_dir_all(&user_directory).map_err(|error| {
        format!(
            "Failed to create portal config directory {}: {error}",
            user_directory.display()
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(&user_directory).map_err(|error| {
        format!(
            "Failed to create temporary portal policy in {}: {error}",
            user_directory.display()
        )
    })?;
    temporary
        .write_all(updated.as_bytes())
        .map_err(|error| format!("Failed to write portal policy: {error}"))?;
    temporary.persist(&target).map_err(|error| {
        format!(
            "Failed to install portal policy {}: {error}",
            target.display()
        )
    })?;

    Ok(target)
}

#[cfg(not(test))]
fn current_desktop() -> Result<String, String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .map_err(|_| "XDG_CURRENT_DESKTOP is not set".to_string())?;
    desktop
        .split(':')
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_ascii_lowercase())
        .ok_or_else(|| "XDG_CURRENT_DESKTOP contains no desktop name".to_string())
}

#[cfg(not(test))]
fn system_policy_directories() -> Vec<PathBuf> {
    let data_directories =
        std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::env::split_paths(&data_directories)
        .map(|directory| directory.join("xdg-desktop-portal"))
        .collect()
}
