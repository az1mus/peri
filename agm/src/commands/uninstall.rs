use crate::adapter::get_adapter;
use crate::error::{AgmError, Result};
use crate::resolver::PackageType;
use crate::types::*;
use std::path::PathBuf;

pub fn execute(package: &str, target: &str, project_dir: Option<PathBuf>) -> Result<()> {
    let dir = project_dir.unwrap_or_else(|| PathBuf::from("."));
    let manifest_path = dir.join("agm.json");

    if !manifest_path.exists() {
        return Err(AgmError::ManifestNotFound);
    }

    let adapter = get_adapter(target)
        .ok_or_else(|| AgmError::Other(format!("unknown target: {}", target)))?;

    let mut manifest = ProjectManifest::load(&manifest_path)?;

    let removed_skills = manifest.skills.remove(package);
    let removed_agents = manifest.agents.remove(package);
    let removed_mcp = manifest.mcp.remove(package);

    if removed_skills.is_none() && removed_agents.is_none() && removed_mcp.is_none() {
        return Err(AgmError::PackageNotInManifest(package.into()));
    }

    let link_name = crate::adapter::symlink_name(package, &[]);

    if removed_skills.is_some() {
        let target_dir = adapter.map_dir(PackageType::Skills, &dir);
        adapter.uninstall(&target_dir, &link_name)?;
    }
    if removed_agents.is_some() {
        let target_dir = adapter.map_dir(PackageType::Agents, &dir);
        adapter.uninstall(&target_dir, &link_name)?;
    }
    if removed_mcp.is_some() {
        let target_dir = adapter.map_dir(PackageType::Mcp, &dir);
        adapter.uninstall(&target_dir, &link_name)?;
    }

    manifest.save(&manifest_path)?;

    let lock_path = dir.join("agm.lock.json");
    if lock_path.exists() {
        let mut lock = LockFile::load(&lock_path)?;
        if let Some(importer) = lock.importers.get_mut(".") {
            importer.skills.remove(package);
            importer.agents.remove(package);
            importer.mcp.remove(package);
        }
        lock.packages
            .retain(|k, _| !k.starts_with(&format!("{}@", package)));
        lock.save(&lock_path)?;
    }

    println!("Uninstalled {}", package);
    Ok(())
}
