use std::collections::HashSet;
#[cfg(not(test))]
use std::io::Write;
#[cfg(not(test))]
use std::path::Path;
use std::path::PathBuf;

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

pub(super) fn find_policy_source(directories: &[PathBuf], desktops: &[String]) -> Option<PathBuf> {
    for directory in directories {
        for desktop in desktops {
            let desktop_policy = directory.join(format!("{desktop}-portals.conf"));
            if desktop_policy.is_file() {
                return Some(desktop_policy);
            }
        }
        let generic_policy = directory.join("portals.conf");
        if generic_policy.is_file() {
            return Some(generic_policy);
        }
    }
    None
}

pub(super) fn parse_desktops(value: &str) -> Vec<String> {
    value
        .split(':')
        .map(str::trim)
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(super) fn build_policy_directories(
    user_config: PathBuf,
    system_config: Vec<PathBuf>,
    user_data: PathBuf,
    system_data: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut seen = HashSet::new();
    push_unique_policy_directory(&mut directories, &mut seen, user_config);
    for directory in system_config {
        push_unique_policy_directory(&mut directories, &mut seen, directory);
    }
    push_unique_directory(
        &mut directories,
        &mut seen,
        PathBuf::from("/etc/xdg-desktop-portal"),
    );
    push_unique_policy_directory(&mut directories, &mut seen, user_data);
    for directory in system_data {
        push_unique_policy_directory(&mut directories, &mut seen, directory);
    }
    push_unique_directory(
        &mut directories,
        &mut seen,
        PathBuf::from("/usr/share/xdg-desktop-portal"),
    );
    directories
}

fn push_unique_policy_directory(
    directories: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    base: PathBuf,
) {
    push_unique_directory(directories, seen, base.join("xdg-desktop-portal"));
}

fn push_unique_directory(
    directories: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    directory: PathBuf,
) {
    if seen.insert(directory.clone()) {
        directories.push(directory);
    }
}

#[cfg(not(test))]
pub(super) fn configure() -> Result<PathBuf, String> {
    let desktops = current_desktops()?;
    let user_directory = dirs::config_dir()
        .ok_or_else(|| "Cannot determine XDG configuration directory".to_string())?
        .join("xdg-desktop-portal");
    let override_directory = std::env::var_os("XDG_DESKTOP_PORTAL_DIR").map(PathBuf::from);
    let directories = match &override_directory {
        Some(directory) => vec![directory.clone()],
        None => policy_directories_from_environment()?,
    };
    let source = find_policy_source(&directories, &desktops).ok_or_else(|| {
        format!(
            "No existing portal policy found for desktops {}",
            desktops.join(":")
        )
    })?;
    let target = if override_directory.is_some() || source.starts_with(&user_directory) {
        source.clone()
    } else {
        user_directory.join(format!("{}-portals.conf", desktops[0]))
    };
    write_updated_policy(&source, &target)?;
    Ok(target)
}

#[cfg(not(test))]
fn current_desktops() -> Result<Vec<String>, String> {
    let value = std::env::var("XDG_CURRENT_DESKTOP")
        .map_err(|_| "XDG_CURRENT_DESKTOP is not set".to_string())?;
    let desktops = parse_desktops(&value);
    if desktops.is_empty() {
        Err("XDG_CURRENT_DESKTOP contains no desktop name".into())
    } else {
        Ok(desktops)
    }
}

#[cfg(not(test))]
fn policy_directories_from_environment() -> Result<Vec<PathBuf>, String> {
    let user_config = dirs::config_dir()
        .ok_or_else(|| "Cannot determine XDG configuration directory".to_string())?;
    let system_config = split_xdg_paths("XDG_CONFIG_DIRS", "/etc/xdg");
    let user_data =
        dirs::data_dir().ok_or_else(|| "Cannot determine XDG data directory".to_string())?;
    let system_data = split_xdg_paths("XDG_DATA_DIRS", "/usr/local/share:/usr/share");
    Ok(build_policy_directories(
        user_config,
        system_config,
        user_data,
        system_data,
    ))
}

#[cfg(not(test))]
fn split_xdg_paths(variable: &str, default: &str) -> Vec<PathBuf> {
    let value = std::env::var_os(variable).unwrap_or_else(|| default.into());
    std::env::split_paths(&value).collect()
}

#[cfg(not(test))]
fn write_updated_policy(source: &Path, target: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(source)
        .map_err(|error| format!("Failed to read portal policy {}: {error}", source.display()))?;
    let updated = set_filechooser_backend(&content);
    let target_directory = target.parent().ok_or_else(|| {
        format!(
            "Portal policy has no parent directory: {}",
            target.display()
        )
    })?;
    std::fs::create_dir_all(target_directory).map_err(|error| {
        format!(
            "Failed to create portal config directory {}: {error}",
            target_directory.display()
        )
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(target_directory).map_err(|error| {
        format!(
            "Failed to create temporary portal policy in {}: {error}",
            target_directory.display()
        )
    })?;
    temporary
        .write_all(updated.as_bytes())
        .map_err(|error| format!("Failed to write portal policy: {error}"))?;
    temporary.persist(target).map_err(|error| {
        format!(
            "Failed to install portal policy {}: {error}",
            target.display()
        )
    })?;
    Ok(())
}
