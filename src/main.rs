use rocket::serde::json::Json;
use rocket::{launch, post, routes};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tokio::{fs, try_join};

#[derive(Deserialize)]
struct Input {
    files: HashMap<String, File>,
    stdin: String,
    code: String,
}

#[derive(Deserialize)]
struct File {
    contents: String,
}

#[derive(Serialize)]
struct Output {
    status: Option<i32>,
    output: String,
}

async fn read_into_output(
    mut stdout: impl AsyncRead + Unpin,
    mut stderr: impl AsyncRead + Unpin,
    output: &mut Vec<u8>,
) -> io::Result<()> {
    let mut current = b'O';
    let mut stdout_buffer = [0; 4096];
    let mut stderr_buffer = [0; 4096];
    let mut append = |buffer: &[u8], t, disable: &mut bool, output: &mut Vec<u8>| {
        if buffer.is_empty() {
            *disable = true;
            return;
        }
        output.reserve(if current != t { 2 } else { 0 } + buffer.len());
        if current != t {
            output.push(0x7F);
            output.push(t);
            current = t;
        }
        for &b in buffer {
            output.push(b);
            if b == 0x7F {
                output.push(0x7F);
            }
        }
    };
    let mut stdout_disabled = false;
    let mut stderr_disabled = false;
    loop {
        if output.len() > 1_000_000 {
            break;
        }
        tokio::select! {
            read = stdout.read(&mut stdout_buffer), if !stdout_disabled => {
                append(&stdout_buffer[..read?], b'O', &mut stdout_disabled, output);
            }
            read = stderr.read(&mut stderr_buffer), if !stderr_disabled => {
                append(&stderr_buffer[..read?], b'E', &mut stderr_disabled, output);
            }
            else => break,
        }
    }
    return Ok(());
}

#[post("/", data = "<input>")]
async fn sandbox(input: Json<Input>) -> io::Result<Json<Output>> {
    let home = tempfile::tempdir()?;
    for (name, File { contents }) in &input.files {
        if !name.is_empty()
            && !name.starts_with('.')
            && name.chars().all(|c| c.is_ascii_alphabetic() || c == '.')
        {
            fs::write(home.path().join(name), contents).await?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Filenames can only contain ASCII alphanumeric characters and dots",
            ));
        }
    }
    let mut private = OsString::from("--private=");
    private.push(home.path());
    let mut child = Command::new("bwrap")
        .arg("--bind")
        .arg(home.path())
        .args(&[
            "/run/sandbox",
            "--ro-bind",
            "/nix/store",
            "/nix/store",
            "--proc",
            "/proc",
            "--dev-bind",
            "/dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--unshare-all",
            "--die-with-parent",
            "--chdir",
            "/run/sandbox",
            "sh",
            "-c",
            &input.code,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let mut output = Vec::new();
    let result = timeout(Duration::from_secs(10), async {
        let ((), (), status) = try_join!(
            stdin.write_all(input.stdin.as_bytes()),
            read_into_output(stdout, stderr, &mut output),
            child.wait(),
        )?;
        io::Result::Ok(status.code())
    })
    .await;
    let status = match result {
        Err(_) => None, // Elapsed, not an error
        Ok(status) => status?,
    };
    Ok(Json(Output {
        output: String::from_utf8_lossy(&output).into_owned(),
        status,
    }))
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![sandbox])
}
