use std::vec::*;
use std::string::*;
use std::option::*;
use std::process::Command;
use std::time::{Duration, SystemTime};
use std::path::Path;

fn match_pattern(s: &str, pattern: &str) -> Option<String> {
    if s.starts_with(pattern) {
        Some(s[pattern.len()..].to_string())
    } else {
        None
    }
}

fn any_older<S: AsRef<std::ffi::OsStr>>(path: &S, last_update: SystemTime) -> bool {
    let p = Path::new(path);
    if p.is_dir() {
        p.read_dir().unwrap().any(|c| any_older(&c.unwrap().path(), last_update))
    } else {
        p.metadata().unwrap().modified().unwrap() > last_update
    }
}

fn main() {
    let mut dirs: Vec<String> = Vec::new();
    let mut command: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    let mut sleep_after_restart: u64 = 1000;
    for input in std::env::args().skip(1) {
        if let Some(dir) = match_pattern(&input, "--dir=") { dirs.push(dir); }
        if let Some(c) = match_pattern(&input, "--command=") { command = Some(c); }
        if let Some(arg) = match_pattern(&input, "--arg=") { args.push(arg); }
        if let Some(sleep_str) = match_pattern(&input, "--sleep_after_restart_millis=") { sleep_after_restart = sleep_str.parse().unwrap(); }
    }
    if command.is_none() || dirs.is_empty() {
        println!("Reruns command if files are modified.\nUsage:");
        println!("--dir= (can repeat)\n--command=\n--arg= (optional, can repeat)");
        println!("--sleep_after_restart_millis= (optional)");
        std::process::exit(0);
    }

    let mut last_update = SystemTime::now();
    let mut child = Command::new(command.clone().unwrap()).args(args.clone()).spawn().expect("failed to run command");

    loop {
        if  dirs.iter().any(|p| any_older(p, last_update)) {
            let _ = child.kill();
            child = Command::new(command.clone().unwrap()).args(args.clone()).
                spawn().expect("failed to run command");
            std::thread::sleep(Duration::from_millis(sleep_after_restart));
            last_update = SystemTime::now();
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}