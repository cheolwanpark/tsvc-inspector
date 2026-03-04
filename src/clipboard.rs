use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Copy)]
struct ClipboardCommand {
    program: &'static str,
    args: &'static [&'static str],
}

const MACOS_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "pbcopy",
    args: &[],
}];

const WINDOWS_COMMANDS: &[ClipboardCommand] = &[ClipboardCommand {
    program: "clip",
    args: &[],
}];

const LINUX_COMMANDS: &[ClipboardCommand] = &[
    ClipboardCommand {
        program: "wl-copy",
        args: &[],
    },
    ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard"],
    },
    ClipboardCommand {
        program: "xsel",
        args: &["--clipboard", "--input"],
    },
];

enum CopyError {
    NotFound,
    Failed(String),
}

pub fn copy_text(text: &str) -> Result<(), String> {
    let commands: &[ClipboardCommand] = if cfg!(target_os = "macos") {
        MACOS_COMMANDS
    } else if cfg!(target_os = "windows") {
        WINDOWS_COMMANDS
    } else {
        LINUX_COMMANDS
    };

    let mut attempted = Vec::new();
    for command in commands {
        attempted.push(command.program);
        match run_copy_command(*command, text) {
            Ok(()) => return Ok(()),
            Err(CopyError::NotFound) => continue,
            Err(CopyError::Failed(message)) => return Err(message),
        }
    }

    Err(format!(
        "clipboard command not found (tried: {})",
        attempted.join(", ")
    ))
}

fn run_copy_command(command: ClipboardCommand, text: &str) -> Result<(), CopyError> {
    let mut child = match Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(CopyError::NotFound),
        Err(err) => {
            return Err(CopyError::Failed(format!(
                "{}: failed to start ({err})",
                command.program
            )));
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        return Err(CopyError::Failed(format!(
            "{}: stdin pipe unavailable",
            command.program
        )));
    };

    if let Err(err) = stdin.write_all(text.as_bytes()) {
        return Err(CopyError::Failed(format!(
            "{}: failed to write clipboard data ({err})",
            command.program
        )));
    }
    drop(stdin);

    let output = child.wait_with_output().map_err(|err| {
        CopyError::Failed(format!(
            "{}: clipboard process failed ({err})",
            command.program
        ))
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let reason = if stderr.is_empty() {
        format!("exit status {}", output.status)
    } else {
        stderr
    };
    Err(CopyError::Failed(format!(
        "{}: clipboard command failed ({reason})",
        command.program
    )))
}
