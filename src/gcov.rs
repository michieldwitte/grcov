use lazy_static::lazy_static;
use semver::Version;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub enum GcovToolError {
    ProcessFailure,
    Failure((String, String, String)),
}

impl fmt::Display for GcovToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            GcovToolError::ProcessFailure => write!(f, "Failed to execute gcov process"),
            GcovToolError::Failure((ref path, ref stdout, ref stderr)) => {
                writeln!(f, "gcov execution failed on {path}")?;
                writeln!(f, "gcov stdout: {stdout}")?;
                writeln!(f, "gcov stderr: {stderr}")
            }
        }
    }
}

fn get_gcov() -> String {
    if let Ok(s) = env::var("GCOV") {
        s
    } else {
        "gcov".to_string()
    }
}

pub fn run_gcov(
    gcno_path: &Path,
    branch_enabled: bool,
    working_dir: &Path,
) -> Result<(), GcovToolError> {
    let mut command = Command::new(get_gcov());
    let command = if branch_enabled {
        command.arg("-b").arg("-c")
    } else {
        &mut command
    };
    let status = command
        .arg(gcno_path)
        .arg("-i") // Generate intermediate gcov format, faster to parse.
        .current_dir(working_dir);

    let output = if let Ok(output) = status.output() {
        output
    } else {
        return Err(GcovToolError::ProcessFailure);
    };

    if !output.status.success() {
        return Err(GcovToolError::Failure((
            gcno_path.to_str().unwrap().to_string(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }

    Ok(())
}

pub fn get_gcov_version() -> &'static Version {
    lazy_static! {
        static ref V: Version = {
            let output = Command::new(get_gcov())
                .arg("--version")
                .output()
                .expect("Failed to execute `gcov`. `gcov` is required (it is part of GCC).");
            assert!(output.status.success(), "`gcov` failed to execute.");
            let output = String::from_utf8(output.stdout).unwrap();
            parse_version(&output)
        };
    }
    &V
}

pub fn get_gcov_output_ext() -> &'static str {
    lazy_static! {
        static ref E: &'static str = {
            let min_ver = Version::new(9, 1, 0);
            if get_gcov_version() >= &min_ver {
                ".gcov.json.gz"
            } else {
                ".gcov"
            }
        };
    }
    &E
}

/// Major version of the GCC that wrote `gcno`, from the version stamp that follows the
/// magic. GCC 7 and newer encode the major as a letter plus a digit, before that it is a
/// single digit followed by the two digits of the minor.
fn gcno_major(gcno_path: &Path) -> Option<u64> {
    let mut header: [u8; 8] = [0; 8];
    File::open(gcno_path).ok()?.read_exact(&mut header).ok()?;

    let stamp: Vec<u8> = match &header[..4] {
        b"oncg" => header[4..].iter().rev().copied().collect(),
        b"gcno" => header[4..].to_vec(),
        _ => return None,
    };

    match stamp[0] {
        b'A'..=b'Z' => {
            Some((stamp[0] - b'A') as u64 * 10 + (stamp[1] as char).to_digit(10)? as u64)
        }
        b'0'..=b'9' => Some((stamp[0] as char).to_digit(10)? as u64),
        _ => None,
    }
}

/// Why the gcov we are about to run cannot read `gcno`, if it cannot. gcov only knows
/// the format of its own major version and, given another one, can spend seconds on it
/// before it fails or even crashes, so those are better skipped than run.
pub fn gcov_cannot_read(gcno_path: &Path) -> Option<String> {
    let major = gcno_major(gcno_path)?;
    let gcov_major = get_gcov_version().major;
    (major != gcov_major).then(|| format!("it was written by GCC {major}, not {gcov_major}"))
}

fn parse_version(gcov_output: &str) -> Version {
    let version = gcov_output
        .split([' ', '\n'])
        .filter_map(|value| Version::parse(value.trim()).ok())
        .next_back();
    assert!(version.is_some(), "no version found for `gcov`.");

    version.unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(
            parse_version("gcov (Ubuntu 4.3.0-12ubuntu2) 4.3.0 20170406"),
            Version::new(4, 3, 0)
        );
        assert_eq!(
            parse_version("gcov (Ubuntu 4.9.0-12ubuntu2) 4.9.0 20170406"),
            Version::new(4, 9, 0)
        );
        assert_eq!(
            parse_version("gcov (Ubuntu 6.3.0-12ubuntu2) 6.3.0 20170406"),
            Version::new(6, 3, 0)
        );
        assert_eq!(parse_version("gcov (GCC) 12.2.0"), Version::new(12, 2, 0));
        assert_eq!(parse_version("gcov (GCC) 12.2.0\r"), Version::new(12, 2, 0));
    }

    #[test]
    fn test_gcno_major() {
        for (name, major) in [
            ("test/reader_gcc-6.gcno", Some(6)),
            ("test/reader_gcc-7.gcno", Some(7)),
            ("test/reader_gcc-8.gcno", Some(8)),
            ("test/reader_gcc-9.gcno", Some(9)),
            ("test/reader_gcc-10.gcno", Some(10)),
            ("test/reader_gcc-11.gcno", Some(11)),
            ("test/reader_gcc-12.gcno", Some(12)),
            ("test/reader_gcc-15.gcno", Some(15)),
            // clang writes a gcno of GCC 11.1
            ("test/reader_clang-22.gcno", Some(11)),
            ("test/reader_gcc-15.gcda", None),
            ("test/does_not_exist.gcno", None),
        ] {
            assert_eq!(gcno_major(Path::new(name)), major, "{name}");
        }
    }
}
