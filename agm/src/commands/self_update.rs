use crate::error::{AgmError, Result};
use std::process::Command;

const INSTALL_SH_URL: &str = "https://raw.githubusercontent.com/konghayao/peri/main/agm/install.sh";
const INSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/konghayao/peri/main/agm/install.ps1";

pub fn execute(force: bool) -> Result<()> {
    if cfg!(windows) {
        let env_setup = if force { "$env:AGM_FORCE='1'; " } else { "" };
        let script = format!("{}irm {} | iex", env_setup, INSTALL_PS1_URL);
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .status()
            .map_err(|e| AgmError::Other(format!("failed to run powershell: {}", e)))?;
        if !status.success() {
            return Err(AgmError::Other("self-update failed".into()));
        }
    } else {
        let env_setup = if force { "AGM_FORCE=1 " } else { "" };
        let script = format!("curl -fsSL {} | {}bash", INSTALL_SH_URL, env_setup);
        let status = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .map_err(|e| AgmError::Other(format!("failed to run curl|bash: {}", e)))?;
        if !status.success() {
            return Err(AgmError::Other("self-update failed".into()));
        }
    }

    Ok(())
}
