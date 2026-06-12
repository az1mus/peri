use crate::adapter::*;
use crate::config::AgmConfig;
use crate::error::{AgmError, Result};
use crate::git;
use crate::registry::RegistryClient;
use crate::resolver::*;
use crate::store::*;
use crate::types::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;

/// Auto-detection results: (skills, agents), each element is a (name, glob) pair, glob is relative path in store
type DetectedTypes = (Vec<(String, String)>, Vec<(String, String)>);

/// Installation context
pub struct InstallContext {
    pub config: AgmConfig,
    pub store: Store,
    pub manifest: ProjectManifest,
    pub lock: Option<LockFile>,
    pub target: String,
    pub project_root: PathBuf,
}

impl InstallContext {
    pub fn new(
        config: AgmConfig,
        manifest: ProjectManifest,
        target: &str,
        project_root: PathBuf,
    ) -> Result<Self> {
        let store = Store::new(config.store_path.clone());
        store.ensure_root()?;

        // Ensure agm temp directory exists (same filesystem as store, for atomic rename)
        let tmp_dir = crate::config::agm_dir().join("tmp");
        std::fs::create_dir_all(&tmp_dir)?;

        let lock_path = project_root.join("agm.lock.json");
        let lock = if lock_path.exists() {
            Some(LockFile::load(&lock_path)?)
        } else {
            None
        };

        Ok(Self {
            config,
            store,
            manifest,
            lock,
            target: target.to_string(),
            project_root,
        })
    }

    /// Create temp directory under ~/.agm/tmp/, ensuring same filesystem as store
    fn temp_dir(&self) -> Result<tempfile::TempDir> {
        let tmp_root = crate::config::agm_dir().join("tmp");
        std::fs::create_dir_all(&tmp_root)?;
        Ok(tempfile::TempDir::new_in(&tmp_root)?)
    }

    /// Install a package directly from git URL (similar to npm install <url>)
    pub fn install_from_git(&mut self, repo_url: &str) -> Result<()> {
        let adapter = get_adapter(&self.target)
            .ok_or_else(|| AgmError::Other(format!("unknown target: {}", self.target)))?;

        // Parse URL → package name
        let (owner, repo) = git::parse_github_url(repo_url)
            .ok_or_else(|| AgmError::Other(format!("unsupported git URL: {}", repo_url)))?;
        let pkg_name = format!("@git/{}/{}", owner, repo);

        // Use ls-remote to get HEAD hash, check if store already has it
        let head_commit = git::resolve_head(repo_url)?;
        let store_path = self.store.git_package_path(repo_url, &head_commit);

        let (skills, agents, store_path, actual_commit, resolution) = if store_path.exists() {
            println!("  (already in store, skipping clone)");
            let pkg_manifest_path = store_path.join("agm.package.json");
            let (skills, agents) = if pkg_manifest_path.exists() {
                let pkg = PackageManifest::load(&pkg_manifest_path)?;
                let skills: Vec<_> = pkg
                    .skills
                    .into_iter()
                    .map(|g| {
                        let n = extract_skill_name(&g);
                        (n, g)
                    })
                    .collect();
                let agents: Vec<_> = pkg
                    .agents
                    .into_iter()
                    .map(|g| {
                        let n = extract_skill_name(&g);
                        (n, g)
                    })
                    .collect();
                (skills, agents)
            } else {
                self.auto_detect_types(&store_path)
            };
            let resolution = Resolution::Git {
                repo: repo_url.to_string(),
                commit: head_commit.clone(),
            };
            (skills, agents, store_path, head_commit.clone(), resolution)
        } else {
            let temp_dir = self.temp_dir()?;
            let cloned_commit = git::clone_head(repo_url, temp_dir.path())?;
            tracing::info!("cloned {} at commit {}", repo_url, &cloned_commit[..12]);

            if cloned_commit != head_commit {
                tracing::warn!(
                    "HEAD changed during clone (expected {}, got {})",
                    &head_commit[..12],
                    &cloned_commit[..12]
                );
            }

            let pkg_manifest_path = temp_dir.path().join("agm.package.json");
            let (skills, agents) = if pkg_manifest_path.exists() {
                let pkg = PackageManifest::load(&pkg_manifest_path)?;
                let skills: Vec<_> = pkg
                    .skills
                    .into_iter()
                    .map(|g| {
                        let n = extract_skill_name(&g);
                        (n, g)
                    })
                    .collect();
                let agents: Vec<_> = pkg
                    .agents
                    .into_iter()
                    .map(|g| {
                        let n = extract_skill_name(&g);
                        (n, g)
                    })
                    .collect();
                (skills, agents)
            } else {
                self.auto_detect_types(temp_dir.path())
            };

            let resolution = Resolution::Git {
                repo: repo_url.to_string(),
                commit: cloned_commit.clone(),
            };
            let store_path = install_to_store(
                &self.store,
                temp_dir.path(),
                &resolution,
                &pkg_name,
                &cloned_commit,
            )?;
            let _ = temp_dir.close();
            (skills, agents, store_path, cloned_commit, resolution)
        };

        let mut installed = Vec::new();

        // Create symlinks for skills
        for (skill_name, skill_glob) in &skills {
            let target_dir = adapter.map_dir(PackageType::Skills, &self.project_root);
            let link_name = symlink_name(skill_name, &[]);
            let store_skill_path = store_path.join(skill_glob);
            if store_skill_path.exists() {
                let parent = store_skill_path.parent().unwrap_or(&store_skill_path);
                adapter
                    .install(parent, &target_dir, &link_name)
                    .map_err(|e| AgmError::Other(format!("symlink skill {}: {}", skill_name, e)))?;
                println!(
                    "  ✓ skill: {} → .{}/skills/{}",
                    skill_name, self.target, link_name
                );
                self.manifest
                    .skills
                    .insert(pkg_name.clone(), actual_commit.clone());
                installed.push((pkg_name.clone(), actual_commit.clone(), resolution.clone()));
            }
        }

        // Create symlinks for agents
        for (agent_name, agent_glob) in &agents {
            let target_dir = adapter.map_dir(PackageType::Agents, &self.project_root);
            let link_name = symlink_name(agent_name, &[]);
            let store_agent_path = store_path.join(agent_glob);
            if store_agent_path.exists() {
                adapter
                    .install(&store_agent_path, &target_dir, &link_name)
                    .map_err(|e| AgmError::Other(format!("symlink agent {}: {}", agent_name, e)))?;
                println!(
                    "  ✓ agent: {} → .{}/agents/{}",
                    agent_name, self.target, link_name
                );
                self.manifest
                    .agents
                    .insert(pkg_name.clone(), actual_commit.clone());
                if !installed.iter().any(|(n, _, _)| n == &pkg_name) {
                    installed.push((pkg_name.clone(), actual_commit.clone(), resolution.clone()));
                }
            }
        }

        if skills.is_empty() && agents.is_empty() {
            println!("No skills or agents found in the repo. If the repo has an agm.package.json, it should declare exports.");
        }

        // Save agm.json
        let manifest_path = self.project_root.join("agm.json");
        self.manifest
            .save(&manifest_path)
            .map_err(|e| AgmError::Other(format!("save agm.json: {}", e)))?;

        // Update lock file
        self.update_lock(&installed)
            .map_err(|e| AgmError::Other(format!("update lock: {}", e)))?;

        adapter
            .post_install()
            .map_err(|e| AgmError::Other(format!("post_install: {}", e)))?;
        Ok(())
    }

    /// Auto-detect skills and agents in a repo (when no agm.package.json)
    /// Returns (name, glob) pairs, glob is relative path in store
    fn auto_detect_types(&self, repo_root: &Path) -> DetectedTypes {
        let mut skills: Vec<(String, String)> = Vec::new();
        let mut agents: Vec<(String, String)> = Vec::new();

        // Detect .{tool}/skills/**/SKILL.md (supports nested categories via recursion)
        for tool_prefix in &[".claude", ".codex", ".copilot", ""] {
            let skills_dir = if tool_prefix.is_empty() {
                repo_root.join("skills")
            } else {
                repo_root.join(tool_prefix).join("skills")
            };
            skills.extend(find_skills_recursive(&skills_dir, repo_root));

            // Detect .{tool}/agents/*.md
            let agents_dir = if tool_prefix.is_empty() {
                repo_root.join("agents")
            } else {
                repo_root.join(tool_prefix).join("agents")
            };
            if let Ok(entries) = std::fs::read_dir(&agents_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                        let name = path.file_stem().unwrap().to_string_lossy().to_string();
                        let prefix = if tool_prefix.is_empty() {
                            "agents".to_string()
                        } else {
                            format!("{}/agents", tool_prefix)
                        };
                        let glob = format!("{}/{}.md", prefix, name);
                        tracing::info!("auto-detected agent: {} ({})", name, glob);
                        agents.push((name, glob));
                    }
                }
            }
        }

        (skills, agents)
    }
}

/// Recursively find directories containing SKILL.md, supporting nested categories
/// (e.g., skills/engineering/grill-me/SKILL.md)
/// Returns (skill_name, path relative to repo_root)
fn find_skills_recursive(base_dir: &Path, repo_root: &Path) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let entries = match std::fs::read_dir(base_dir) {
        Ok(e) => e,
        Err(_) => return result,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let rel = skill_md.strip_prefix(repo_root).unwrap_or(&skill_md);
            let glob = rel.to_string_lossy().to_string();
            tracing::info!("auto-detected skill: {} ({})", name, glob);
            result.push((name, glob));
        } else {
            // Recurse into subdirectories (e.g., skills/engineering/, skills/productivity/)
            result.extend(find_skills_recursive(&path, repo_root));
        }
    }
    result
}

impl InstallContext {
    pub fn install_all(&mut self) -> Result<()> {
        let adapter = get_adapter(&self.target)
            .ok_or_else(|| AgmError::Other(format!("unknown target: {}", self.target)))?;

        let deps = collect_dependencies(&self.manifest);

        if deps.is_empty() {
            tracing::info!("no dependencies to install");
            return Ok(());
        }

        let registry_url = self
            .manifest
            .registry
            .as_deref()
            .unwrap_or(&self.config.default_registry);
        let registry_client = RegistryClient::new(registry_url, self.config.registry_token.clone());

        let types = [PackageType::Skills, PackageType::Agents, PackageType::Mcp];
        let mut installed_packages: Vec<(String, String, Resolution)> = Vec::new();
        let rt = Runtime::new()?;

        for typ in &types {
            let deps_of_type: Vec<_> = deps.iter().filter(|(_, _, t)| t == typ).collect();

            for (name, version, _) in &deps_of_type {
                let lock_version: String;
                let resolution;

                if is_git_dep(name) {
                    validate_commit_hash(version)?;
                    lock_version = version.clone();

                    let pkg_key = format!("{}@{}", name, lock_version);
                    if let Some(lock) = &self.lock {
                        if lock.packages.contains_key(&pkg_key) {
                            continue;
                        }
                    }

                    let temp_dir = self.temp_dir()?;
                    let repo_url =
                        format!("https://github.com/{}", name.trim_start_matches("@git/"));
                    git::clone_at_commit(&repo_url, version, temp_dir.path())?;

                    resolution = Resolution::Git {
                        repo: repo_url,
                        commit: version.clone(),
                    };

                    install_to_store(&self.store, temp_dir.path(), &resolution, name, version)?;
                    let _ = temp_dir.close();
                } else {
                    let resolved_version =
                        rt.block_on(resolve_registry_version(&registry_client, name, version))?;
                    lock_version = resolved_version.clone();

                    let pkg_key = format!("{}@{}", name, lock_version);
                    if let Some(lock) = &self.lock {
                        if lock.packages.contains_key(&pkg_key) {
                            continue;
                        }
                    }

                    let version_meta =
                        rt.block_on(registry_client.get_version(name, &resolved_version))?;

                    let temp_dir = self.temp_dir()?;
                    let tarball_path = temp_dir.path().join("pkg.tar.gz");
                    rt.block_on(registry_client.download_tarball(
                        name,
                        &version_meta.tarball,
                        &tarball_path,
                    ))?;

                    let extract_dir = temp_dir.path().join("extracted");
                    std::fs::create_dir(&extract_dir)?;
                    extract_tarball(&tarball_path, &extract_dir)?;

                    resolution = Resolution::Registry {
                        integrity: version_meta.integrity.clone(),
                    };

                    install_to_store(
                        &self.store,
                        &extract_dir,
                        &resolution,
                        name,
                        &resolved_version,
                    )?;
                }

                // Create symlink
                let target_dir = adapter.map_dir(*typ, &self.project_root);
                let link_name = symlink_name(name, &[]);
                let store_path = match &resolution {
                    Resolution::Git { repo, commit, .. } => {
                        self.store.git_package_path(repo, commit)
                    }
                    Resolution::Registry { .. } => {
                        self.store.registry_package_path(name, &lock_version)
                    }
                };
                adapter.install(&store_path, &target_dir, &link_name)?;

                installed_packages.push((name.clone(), lock_version, resolution));
            }
        }

        adapter.post_install()?;
        self.update_lock(&installed_packages)
    }

    fn update_lock(&self, installed: &[(String, String, Resolution)]) -> Result<()> {
        let mut lock = self.lock.clone().unwrap_or_else(|| LockFile {
            lockfile_version: 1,
            importers: BTreeMap::new(),
            packages: BTreeMap::new(),
        });

        let importer = lock
            .importers
            .entry(".".into())
            .or_insert_with(|| LockImporter {
                skills: BTreeMap::new(),
                agents: BTreeMap::new(),
                mcp: BTreeMap::new(),
            });

        for (name, version, _) in installed {
            let dep = LockDependency {
                version: version.clone(),
            };
            if self.manifest.skills.contains_key(name) {
                importer.skills.insert(name.clone(), dep);
            } else if self.manifest.agents.contains_key(name) {
                importer.agents.insert(name.clone(), dep);
            } else if self.manifest.mcp.contains_key(name) {
                importer.mcp.insert(name.clone(), dep);
            }
        }

        for (name, version, resolution) in installed {
            let pkg_key = format!("{}@{}", name, version);
            lock.packages
                .entry(pkg_key)
                .or_insert_with(|| LockedPackage {
                    resolution: resolution.clone(),
                    targets: vec![self.target.clone()],
                });
        }

        let lock_path = self.project_root.join("agm.lock.json");
        lock.save(&lock_path)?;
        Ok(())
    }
}

/// Extract .tar.gz tarball
fn extract_tarball(tarball_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(tarball_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(dest)?;
    Ok(())
}

/// Extract skill/agent name from a glob path (e.g., ".claude/skills/interview/SKILL.md" → "interview")
fn extract_skill_name(glob: &str) -> String {
    let parts: Vec<&str> = glob.split('/').collect();
    // Find the part after "skills" or "agents"
    for (i, part) in parts.iter().enumerate() {
        if (*part == "skills" || *part == "agents") && i + 1 < parts.len() {
            return parts[i + 1].to_string();
        }
    }
    // fallback: use the last meaningful directory name
    parts
        .iter()
        .rev()
        .find(|p| !p.ends_with(".md") && **p != "SKILL.md")
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".into())
}
