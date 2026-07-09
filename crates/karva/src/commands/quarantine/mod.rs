mod list;

use anyhow::Result;
use karva_cli::{QuarantineAction, QuarantineCommand};
use karva_logging::Printer;

use crate::ExitStatus;
use crate::utils::cwd;

pub fn quarantine(args: &QuarantineCommand) -> Result<ExitStatus> {
    let cwd = cwd()?;

    let printer = Printer::default();
    let mut stdout = printer.stream_for_message().lock();

    match args.action {
        QuarantineAction::List => list::list(&cwd, &mut stdout),
    }
}
