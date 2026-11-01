//! Background helpers must never open a console over the desktop reader.
use std::process::Command;

pub(crate) fn background(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    // Keep one constructor on all platforms.
    let _ = &mut command;
    command
}
