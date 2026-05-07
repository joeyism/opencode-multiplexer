use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn pick_directory(maxdepth: u32) -> anyhow::Result<Option<PathBuf>> {
    let _capture_path = std::env::temp_dir().join(format!(
        "ocmux-rs-fzf-{}.txt",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let search_paths: Vec<PathBuf> = [
        format!("{home}/Programming"),
        format!("{home}/repos"),
        format!("{home}/projects"),
        format!("{home}/code"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.is_dir())
    .collect();

    let search_paths = if search_paths.is_empty() {
        vec![PathBuf::from(&home)]
    } else {
        search_paths
    };

    let mut find_args = vec![];
    for path in &search_paths {
        find_args.push(path.display().to_string());
    }
    find_args.extend_from_slice(&[
        "-maxdepth".into(),
        maxdepth.to_string().into(),
        "-name".into(),
        ".git".into(),
    ]);

    let find_output = Command::new("find")
        .args(&find_args)
        .stderr(Stdio::null())
        .output()?;

    let repos: Vec<String> = String::from_utf8_lossy(&find_output.stdout)
        .lines()
        .filter_map(|line| line.strip_suffix("/.git"))
        .map(|s| s.to_string())
        .collect();

    if repos.is_empty() {
        anyhow::bail!("no git repos found")
    }

    let mut fzf = Command::new("fzf")
        .arg("--prompt=Select repo: ")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = fzf.stdin.take() {
        use std::io::Write;
        for repo in &repos {
            let _ = writeln!(stdin, "{repo}");
        }
    }

    let output = fzf.wait_with_output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        return Ok(None);
    }

    Ok(Some(PathBuf::from(selected)))
}
