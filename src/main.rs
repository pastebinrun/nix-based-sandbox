use rocket::serde::json::Json;
use rocket::{launch, post, routes};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tokio::{fs, try_join};

#[derive(Deserialize)]
struct Input {
    files: Vec<File>,
    stdin: String,
    code: String,
}

#[derive(Deserialize)]
struct File {
    name: String,
    contents: String,
}

#[derive(Serialize)]
struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn read_limited(f: impl AsyncReadExt + Unpin, out: &mut Vec<u8>) -> io::Result<()> {
    f.take(1_000_000).read_to_end(out).await?;
    Ok(())
}

#[post("/", data = "<input>")]
async fn sandbox(input: Json<Input>) -> io::Result<Json<Output>> {
    let home = tempfile::tempdir()?;
    for File { name, contents } in &input.files {
        fs::write(home.path().join(name), contents).await?;
    }
    let mut private = OsString::from("--private=");
    private.push(home.path());
    let mut child = Command::new("bwrap")
        .arg("--bind")
        .arg(home.path())
        .args(&[
            "/home/sandbox",
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
            "/home/sandbox",
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
    let mut stdout_out = Vec::new();
    let mut stderr_out = Vec::new();
    let result = timeout(Duration::from_secs(10), async {
        let ((), (), (), status) = try_join!(
            stdin.write_all(input.stdin.as_bytes()),
            read_limited(stdout, &mut stdout_out),
            read_limited(stderr, &mut stderr_out),
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
        stdout: String::from_utf8_lossy(&stdout_out).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_out).into_owned(),
        status,
    }))
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![sandbox])
}
