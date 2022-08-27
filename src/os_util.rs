use std::ffi::OsStr;
use std::path::Path;

use crate::somehow::{somehow as anyhow, Context, Error};

pub fn get_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let uname = rustix::process::uname();
        if let Ok(node_name) = uname.nodename().to_str() {
            if !node_name.is_empty() {
                return Some(node_name.to_owned());
            }
        }
    }
    None
}

pub struct Uids {
    pub real_user: u64,
    pub group: u64,
}

pub fn get_uids() -> Uids {
    Uids {
        real_user: rustix::process::getuid().as_raw().into(),
        group: rustix::process::getgid().as_raw().into(),
    }
}

pub fn get_timezone() -> String {
    try_get_timezone().unwrap_or_else(|| String::from("Etc/UTC"))
}

// This function is private and using `Option` rather than `Result` due to the
// odd multi-error warning here.
fn try_get_timezone() -> Option<String> {
    let mut errors: Vec<Error> = Vec::new();

    match std::env::var("TZ").context("Failed to read 'TZ' environment variable") {
        Ok(s) => return Some(s.trim().to_owned()),
        Err(e) => errors.push(e),
    }

    match std::fs::read_to_string("/etc/timezone")
        .context("Failed to read /etc/timezone (which is not present on many systems)")
    {
        Ok(s) => return Some(s.trim().to_owned()),
        Err(e) => errors.push(e),
    }

    match std::fs::read_link("/etc/localtime")
        .context("Failed to read symlink target for '/etc/localtime'")
    {
        Ok(target) => match timezone_from_localtime_target(&target) {
            Some(s) => return Some(s),
            None => errors.push(anyhow!("Could not extract time zone name: '/etc/localtime' symlink did not point into a 'zoneinfo' directory, got: {target:?}"))
        },
        Err(e) => errors.push(e),
    }

    println!("Warning: Falling back to UTC");
    for e in errors {
        println!("{:#}", e);
    }

    None
}

/// Returns the name of a time zone from a filesystem path inside a timezone
/// database directory.
fn timezone_from_localtime_target(path: &Path) -> Option<String> {
    // Debian 11 has its timezone db at '/usr/share/zoneinfo/'.
    // Mac OS 12 has its timezone db at '/var/db/timezone/zoneinfo/'.
    let mut iter = path.iter();
    loop {
        match iter.next() {
            Some(component) if component == "zoneinfo" => {
                let tz = iter.collect::<Vec<_>>().join(OsStr::new("/"));
                return tz.to_str().map(|tz| tz.to_owned());
            }
            Some(_) => continue,
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn get_hostname() {
        println!("Hostname: {:?}", super::get_hostname());
        // Yay, didn't crash!
    }

    #[test]
    fn timezone_from_localtime_target() {
        use super::timezone_from_localtime_target as tz;
        assert_eq!(
            Some(String::from("America/Los_Angeles")),
            tz(Path::new("/var/db/timezone/zoneinfo/America/Los_Angeles"))
        );
        assert_eq!(
            Some(String::from("America/Argentina/Ushuaia")),
            tz(Path::new("/usr/share/zoneinfo/America/Argentina/Ushuaia"))
        );
        assert_eq!(
            Some(String::from("UTC")),
            tz(Path::new("../usr/share/zoneinfo/UTC"))
        );
        assert_eq!(None, tz(Path::new("/usr/share/tz/UTC")));
    }

    #[test]
    fn try_get_timezone() {
        let timezone = super::try_get_timezone();
        println!("Timezone: {timezone:?}");
        assert_ne!(None, timezone);
    }
}
