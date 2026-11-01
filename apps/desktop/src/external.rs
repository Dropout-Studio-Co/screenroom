use std::process::Command;

pub fn open_url(address: &str) -> std::io::Result<()> {
    if !(address.starts_with("https://")
        || address.starts_with("http://")
        || address.starts_with("ftp://"))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "only web and ftp addresses are handed to the system",
        ));
    }
    if let Ok(log) = std::env::var("SCREENROOM_OPEN_LOG")
        && !log.trim().is_empty()
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        writeln!(file, "{address}")?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("cmd");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    command.args(["/C", "start", ""]);
    command.arg(address).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::open_url;

    #[test]
    fn only_web_and_ftp_addresses_reach_the_system() {
        for refused in [
            "file:///etc/passwd",
            "magnet:?xt=urn:btih:00",
            "javascript:alert(1)",
            "",
        ] {
            assert!(
                open_url(refused).is_err(),
                "{refused} must never be handed to the system"
            );
        }
    }

    #[test]
    fn the_log_hook_records_the_exact_address_without_launching_anything() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("opened.txt");
        let address = "https://example.org/films?q=pather";
        let previous = std::env::var_os("SCREENROOM_OPEN_LOG");
        unsafe {
            std::env::set_var("SCREENROOM_OPEN_LOG", &log);
        }
        let outcome = open_url(address);
        match previous {
            Some(value) => unsafe {
                std::env::set_var("SCREENROOM_OPEN_LOG", value);
            },
            None => unsafe {
                std::env::remove_var("SCREENROOM_OPEN_LOG");
            },
        }
        outcome.expect("recorded");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("{address}\n")
        );
    }
}
