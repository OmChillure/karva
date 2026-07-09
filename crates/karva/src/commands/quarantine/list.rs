use std::io::Write;

use anyhow::Result;
use camino::Utf8Path;
use karva_cache::{CACHE_DIR, read_quarantine};

use crate::ExitStatus;

pub(super) fn list(cwd: &Utf8Path, stdout: &mut impl Write) -> Result<ExitStatus> {
    let cache_dir = cwd.join(CACHE_DIR);
    let quarantine = read_quarantine(&cache_dir)?;

    if quarantine.is_empty() {
        writeln!(stdout, "No quarantined tests.")?;
        return Ok(ExitStatus::Success);
    }

    for entry in quarantine.tests() {
        writeln!(stdout, "{} ({})", entry.name, entry.reason)?;
    }

    Ok(ExitStatus::Success)
}
