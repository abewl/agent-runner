//! PID file read/liveness helpers shared by `runner daemon start|stop|status`.

use std::fs;
use std::path::Path;

/// True if a process with the given pid exists and is signalable by us.
/// Uses `kill(pid, 0)`, which sends no actual signal — it only checks
/// existence/permission (see `man 2 kill`).
pub fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Reads and parses the pid file at `path`, if present and well-formed.
pub fn read_pid_file(path: &Path) -> Option<i32> {
    let contents = fs::read_to_string(path).ok()?;
    contents.trim().parse::<i32>().ok()
}

/// Removes the pid file at `path`, ignoring a "does not exist" error.
pub fn remove_pid_file(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn process_alive_true_for_own_pid() {
        let own_pid = std::process::id() as i32;
        assert!(process_alive(own_pid));
    }

    #[test]
    fn process_alive_false_for_a_reaped_child_pid() {
        // Spawn a trivial child, wait for it to exit and be reaped, then its
        // pid is guaranteed not to refer to a live process anymore.
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("failed to spawn `true`");
        let pid = child.id() as i32;
        child.wait().expect("failed to wait for child");
        assert!(!process_alive(pid));
    }

    #[test]
    fn read_pid_file_parses_valid_content() {
        let dir = std::env::temp_dir().join(format!("runner-pid-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("valid.pid");
        let mut f = File::create(&path).unwrap();
        writeln!(f, "4242").unwrap();

        assert_eq!(read_pid_file(&path), Some(4242));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_pid_file_none_for_missing_file() {
        let path = std::env::temp_dir().join("runner-pid-test-does-not-exist.pid");
        assert_eq!(read_pid_file(&path), None);
    }

    #[test]
    fn read_pid_file_none_for_malformed_content() {
        let dir =
            std::env::temp_dir().join(format!("runner-pid-test-malformed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.pid");
        let mut f = File::create(&path).unwrap();
        writeln!(f, "not-a-pid").unwrap();

        assert_eq!(read_pid_file(&path), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_pid_file_is_a_noop_when_absent() {
        let path = std::env::temp_dir().join("runner-pid-test-remove-absent.pid");
        // Should not panic even though the file was never created.
        remove_pid_file(&path);
    }
}
