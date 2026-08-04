use chrono::{Local, NaiveDateTime, TimeZone};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProcess {
    pub pid: u32,
    pub session_id: Option<String>,
    pub start_time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedServeProcess {
    pub pid: u32,
    pub port: u16,
    pub start_time: i64,
}

pub fn scan_processes() -> anyhow::Result<Vec<ParsedProcess>> {
    let output = Command::new("ps").args(["-eo", "pid,args"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(parse_process_line)
        .map(|mut process| {
            process.start_time = get_process_start_time_epoch_ms(process.pid).unwrap_or(0);
            process
        })
        .collect())
}

pub fn scan_serve_processes() -> anyhow::Result<Vec<ParsedServeProcess>> {
    let output = Command::new("ps").args(["-eo", "pid,args"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(parse_serve_process_line)
        .map(|mut process| {
            process.start_time = get_process_start_time_epoch_ms(process.pid).unwrap_or(0);
            process
        })
        .collect())
}

fn is_opencode_binary(token: &str) -> bool {
    let basename = token.rsplit('/').next().unwrap_or(token);
    basename == "opencode" || basename == ".opencode"
}

pub fn parse_serve_process_line(line: &str) -> Option<ParsedServeProcess> {
    let mut parts = line.split_whitespace();
    let pid: u32 = parts.next()?.parse().ok()?;
    let tokens: Vec<&str> = parts.collect();
    if tokens.is_empty() {
        return None;
    }

    let opencode_index = if is_opencode_binary(tokens[0]) {
        Some(0)
    } else if matches!(tokens[0], "node" | "bun" | "deno")
        && tokens.get(1).is_some_and(|token| is_opencode_binary(token))
    {
        Some(1)
    } else {
        None
    }?;

    if tokens.get(opencode_index + 1) != Some(&"serve") {
        return None;
    }

    for window in tokens[opencode_index + 2..].windows(2) {
        if window[0] == "--port" {
            let port = window[1].parse().ok()?;
            return Some(ParsedServeProcess {
                pid,
                port,
                start_time: 0,
            });
        }
    }

    None
}

pub fn parse_process_line(line: &str) -> Option<ParsedProcess> {
    let mut parts = line.split_whitespace();
    let pid: u32 = parts.next()?.parse().ok()?;
    let tokens: Vec<&str> = parts.collect();
    if tokens.is_empty() {
        return None;
    }

    let opencode_index = if is_opencode_binary(tokens[0]) {
        Some(0)
    } else if matches!(tokens[0], "node" | "bun" | "deno")
        && tokens.get(1).is_some_and(|token| is_opencode_binary(token))
    {
        Some(1)
    } else {
        None
    }?;

    if tokens.get(opencode_index + 1) == Some(&"serve") {
        return None;
    }

    let mut session_id = None;
    for window in tokens[opencode_index + 1..].windows(2) {
        if window[0] == "-s" {
            session_id = Some(window[1].to_string());
            break;
        }
    }

    Some(ParsedProcess {
        pid,
        session_id,
        start_time: 0,
    })
}

pub fn get_process_start_time_epoch_ms(pid: u32) -> Option<i64> {
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lstart = stdout.trim();
    if lstart.is_empty() {
        return None;
    }

    // Format: "Wed Jul 22 17:33:10 2026" (day may be space-padded: "Jul  2")
    let normalized = lstart.split_whitespace().collect::<Vec<_>>().join(" ");
    let naive = NaiveDateTime::parse_from_str(&normalized, "%a %b %d %H:%M:%S %Y").ok()?;
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
}
