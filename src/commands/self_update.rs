//! `refs self update`: install the latest release over this one, or with `--check` only say
//! whether there is one (exit 3, as `sync --check` does for a project that is out of date).

use crate::self_update::Updater;
use crate::ui::{Console, EXIT_OUT_OF_DATE, fail};

pub(crate) fn run(updater: &dyn Updater, check: bool, console: &mut Console) -> u8 {
    if check {
        return match updater.is_update_available() {
            Ok(true) => {
                console
                    .problem("out of date: a newer release is available; run `refs self update`");
                EXIT_OUT_OF_DATE
            }
            Ok(false) => {
                console.status("up to date");
                0
            }
            Err(e) => fail(e, console),
        };
    }
    match updater.install() {
        Ok(Some(installed)) => {
            let from = installed
                .from
                .map_or(String::new(), |v| format!(" from {v}"));
            console.status(format_args!("updated refs{from} to {}", installed.to));
            0
        }
        Ok(None) => {
            console.status("up to date");
            0
        }
        Err(e) => fail(e, console),
    }
}
