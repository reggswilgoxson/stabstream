//! Detects circular path-dependencies among workspace members at test time,
//! catching cycles before they cause cryptic compiler errors.
//!
//! Only `[dependencies]` and `[build-dependencies]` sections are examined;
//! `[dev-dependencies]` cycles are benign (cargo resolves them correctly).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Cargo.toml parser (no external deps — pure std)
// ---------------------------------------------------------------------------

#[derive(PartialEq)]
enum Section {
    Other,
    Package,
    Deps,
}

fn classify_section(header: &str) -> Section {
    let h = header.to_ascii_lowercase();
    if h == "package" {
        return Section::Package;
    }
    // Match [dependencies], [build-dependencies], and scoped variants like
    // [target.'cfg(unix)'.dependencies].
    if h == "dependencies"
        || h == "build-dependencies"
        || h.ends_with(".dependencies")
        || h.ends_with(".build-dependencies")
    {
        return Section::Deps;
    }
    Section::Other
}

/// Returns `(package_name, vec_of_canonicalized_path_dep_dirs)`.
fn parse_cargo_toml(path: &Path) -> (String, Vec<PathBuf>) {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let crate_dir = path.parent().unwrap();

    let mut package_name = String::new();
    let mut path_deps: Vec<PathBuf> = Vec::new();
    let mut section = Section::Other;

    for line in text.lines() {
        let trimmed = line.trim();

        // Section headers: single [foo] not [[foo]].
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            let header = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim();
            section = classify_section(header);
            continue;
        }

        match section {
            Section::Package if package_name.is_empty() => {
                // Match `name = "foo"` (possibly with extra spaces).
                // Skip `name.workspace = true`.
                if trimmed.starts_with("name") {
                    let after = trimmed["name".len()..].trim_start();
                    if !after.starts_with('.') {
                        if let Some(val) = after.strip_prefix('=') {
                            let v = val.trim().trim_matches('"');
                            if !v.is_empty() {
                                package_name = v.to_string();
                            }
                        }
                    }
                }
            }
            Section::Deps => {
                // Match `path = "..."` wherever it appears on the line
                // (inline table or standalone value).
                if let Some(pos) = trimmed.find("path") {
                    let rest = trimmed[pos + 4..].trim_start();
                    if let Some(rest) = rest.strip_prefix('=') {
                        let val = rest.trim();
                        if val.starts_with('"') {
                            let inner = &val[1..];
                            if let Some(end) = inner.find('"') {
                                let rel = &inner[..end];
                                if let Ok(abs) = crate_dir.join(rel).canonicalize() {
                                    path_deps.push(abs);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (package_name, path_deps)
}

/// Walk up from `start` until we find a Cargo.toml containing `[workspace]`.
fn find_workspace_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            let text = std::fs::read_to_string(&candidate).unwrap_or_default();
            if text.contains("[workspace]") {
                return dir;
            }
        }
        assert!(dir.pop(), "workspace root not found above {}", start.display());
    }
}

/// Parse the `members = [...]` array from a workspace Cargo.toml.
fn read_workspace_members(root: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let mut in_members = false;
    let mut members = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            in_members = true;
        }
        if in_members {
            // Quoted member entry: "crates/foo" or "crates/foo",
            if trimmed.starts_with('"') {
                let s = trimmed.trim_matches(',').trim_matches('"');
                if !s.is_empty() {
                    members.push(root.join(s));
                }
            }
            // End of array
            if trimmed.contains(']') && !trimmed.starts_with("members") {
                break;
            }
        }
    }

    members
}

// ---------------------------------------------------------------------------
// DFS cycle detection
// ---------------------------------------------------------------------------

/// Returns the cycle path (node names) if a back-edge is discovered.
fn dfs_find_cycle<'a>(
    node: &'a str,
    graph: &'a HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> Option<Vec<String>> {
    // Already fully explored — definitely no cycle reachable from here.
    if visited.contains(node) {
        return None;
    }
    // Back edge — cycle detected.
    if let Some(idx) = stack.iter().position(|s| s == node) {
        let mut cycle = stack[idx..].to_vec();
        cycle.push(node.to_string());
        return Some(cycle);
    }

    stack.push(node.to_string());
    if let Some(deps) = graph.get(node) {
        for dep in deps {
            if let Some(cycle) = dfs_find_cycle(dep, graph, visited, stack) {
                return Some(cycle);
            }
        }
    }
    stack.pop();
    visited.insert(node.to_string());
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify the DFS correctly identifies a known cycle: A → B → C → A.
#[test]
fn dfs_detects_known_cycle() {
    let graph: HashMap<String, Vec<String>> = [
        ("A".to_string(), vec!["B".to_string()]),
        ("B".to_string(), vec!["C".to_string()]),
        ("C".to_string(), vec!["A".to_string()]),
    ]
    .into_iter()
    .collect();

    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let cycle = dfs_find_cycle("A", &graph, &mut visited, &mut stack);
    assert!(cycle.is_some(), "expected cycle to be detected");
    let path = cycle.unwrap();
    assert!(
        path.windows(2).any(|w| w[0] == "C" && w[1] == "A"),
        "cycle path should contain C → A back-edge, got: {:?}",
        path
    );
}

/// Verify the DFS does not report a cycle for a simple DAG.
#[test]
fn dfs_clean_dag() {
    let graph: HashMap<String, Vec<String>> = [
        ("core".to_string(), vec![]),
        ("dem".to_string(), vec!["core".to_string()]),
        ("sim".to_string(), vec!["core".to_string(), "dem".to_string()]),
    ]
    .into_iter()
    .collect();

    let mut visited = HashSet::new();
    for node in graph.keys() {
        let mut stack = Vec::new();
        assert!(
            dfs_find_cycle(node, &graph, &mut visited, &mut stack).is_none(),
            "no cycle expected in clean DAG"
        );
    }
}

#[test]
fn workspace_has_no_circular_dependencies() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = find_workspace_root(&manifest_dir);
    let member_paths = read_workspace_members(&root);

    // Map canonicalized member directory → crate name.
    let mut dir_to_name: HashMap<PathBuf, String> = HashMap::new();
    for member_path in &member_paths {
        let cargo_toml = member_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }
        if let Ok(canonical) = member_path.canonicalize() {
            let (name, _) = parse_cargo_toml(&cargo_toml);
            if !name.is_empty() {
                dir_to_name.insert(canonical, name);
            }
        }
    }

    // Build directed graph: crate name → workspace dep names.
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (dir, name) in &dir_to_name {
        let cargo_toml = dir.join("Cargo.toml");
        let (_, dep_paths) = parse_cargo_toml(&cargo_toml);
        let deps: Vec<String> = dep_paths
            .iter()
            .filter_map(|p| dir_to_name.get(p))
            .cloned()
            .collect();
        graph.insert(name.clone(), deps);
    }

    // DFS from every node; accumulate all cycles before asserting.
    let mut visited: HashSet<String> = HashSet::new();
    let mut all_cycles: Vec<String> = Vec::new();

    for node in graph.keys() {
        let mut stack = Vec::new();
        if let Some(cycle) = dfs_find_cycle(node, &graph, &mut visited, &mut stack) {
            all_cycles.push(cycle.join(" → "));
        }
    }

    assert!(
        all_cycles.is_empty(),
        "circular workspace dependencies detected:\n{}",
        all_cycles.join("\n"),
    );
}
