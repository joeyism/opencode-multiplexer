pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            if let Err(e) = cb.set_text(text.to_string()) {
                fallback_copy(text)
                    .map_err(|fe| anyhow::anyhow!("arboard failed: {e}, fallback failed: {fe}"))
            } else {
                Ok(())
            }
        }
        Err(e) => fallback_copy(text)
            .map_err(|fe| anyhow::anyhow!("arboard init failed: {e}, fallback failed: {fe}")),
    }
}

#[cfg(target_os = "macos")]
fn fallback_copy(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("pbcopy exited with status {status}")
    }
}

#[cfg(not(target_os = "macos"))]
fn fallback_copy(_text: &str) -> anyhow::Result<()> {
    anyhow::bail!("no fallback clipboard provider for this platform")
}
